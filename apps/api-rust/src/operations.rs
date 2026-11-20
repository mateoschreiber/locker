use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;
use sqlx::{postgres::PgRow, query, query_scalar, PgPool, Row};
use uuid::Uuid;

use crate::admin::ListParams;
use crate::audit::{admin_actor, audit};
use crate::dump::dump_row;
use crate::entities::{uuid_field, AUTHORIZATIONS, LOANS, OPERATIONS};
use crate::error::ApiError;
use crate::json::ValidJson;

pub async fn list_authorizations(
    State(pool): State<PgPool>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, ApiError> {
    crate::admin::list(&AUTHORIZATIONS, pool, params).await
}

/// POST /authorizations — tool, user and branch UUIDs validated; FKs enforced by PostgreSQL.
pub async fn create_authorization(
    State(pool): State<PgPool>,
    ValidJson(payload): ValidJson,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let tool_id = uuid_field(&payload, "tool_id")?;
    let user_id = uuid_field(&payload, "user_id")?;
    let branch_id = uuid_field(&payload, "branch_id")?;
    let mut tx = pool.begin().await?;
    let row: PgRow = query(
        "INSERT INTO authorizations (id, tool_id, user_id, branch_id, status)
         VALUES ($1, $2, $3, $4, 'PENDING') RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(tool_id)
    .bind(user_id)
    .bind(branch_id)
    .fetch_one(&mut *tx)
    .await?;
    let authorization_id: Uuid = row.try_get("id")?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "AUTHORIZATION_CREATED",
        "Authorization",
        &authorization_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(dump_row(&row))))
}

/// POST /authorizations/{id}/approve — the conditional UPDATE makes the
/// PENDING→APPROVED transition atomic under concurrency.
pub async fn approve_authorization(
    State(pool): State<PgPool>,
    Path(authorization_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = pool.begin().await?;
    let row: Option<PgRow> = query(
        "UPDATE authorizations SET status = 'APPROVED', approved_at = now()
         WHERE id = $1 AND status = 'PENDING' RETURNING *",
    )
    .bind(authorization_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::conflict("La autorización no está pendiente"));
    };
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "AUTHORIZATION_APPROVED",
        "Authorization",
        &authorization_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(dump_row(&row)))
}

/// POST /authorizations/{id}/cancel
pub async fn cancel_authorization(
    State(pool): State<PgPool>,
    Path(authorization_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = pool.begin().await?;
    let row: Option<PgRow> = query(
        "UPDATE authorizations SET status = 'CANCELLED'
         WHERE id = $1 AND status IN ('PENDING', 'APPROVED') RETURNING *",
    )
    .bind(authorization_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::conflict("La autorización no puede cancelarse"));
    };
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "AUTHORIZATION_CANCELLED",
        "Authorization",
        &authorization_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(dump_row(&row)))
}

/// POST /authorizations/{id}/checkout — creates loan + operation atomically.
///
/// Concurrency (audit #7): FOR UPDATE on the authorization serializes retries;
/// FOR UPDATE on the tool plus the active-loan check guarantees a single
/// pending/active loan per tool. A retry after commit returns the existing
/// operation (idempotency, audit #6). MQTT publication and confirmation are
/// Phase 3; the operation stays PENDING, which is the honest requested state.
pub async fn checkout(
    State(pool): State<PgPool>,
    Path(authorization_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let mut tx = pool.begin().await?;
    let authorization: Option<PgRow> =
        query("SELECT * FROM authorizations WHERE id = $1 FOR UPDATE")
            .bind(authorization_id)
            .fetch_optional(&mut *tx)
            .await?;
    let authorization = match authorization {
        Some(row) => row,
        None => return Err(ApiError::conflict("Se requiere una autorización aprobada")),
    };
    let status: String = authorization.try_get("status")?;
    let tool_id: Uuid = authorization.try_get("tool_id")?;
    if status != "APPROVED" && status != "CONSUMED" {
        return Err(ApiError::conflict("Se requiere una autorización aprobada"));
    }
    if status == "CONSUMED" {
        // Idempotent retry: return the operation already created for this authorization.
        let loan_id: Option<Uuid> =
            query_scalar("SELECT id FROM loans WHERE authorization_id = $1")
                .bind(authorization_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(loan_id) = loan_id else {
            return Err(ApiError::conflict("Se requiere una autorización aprobada"));
        };
        let operation: Option<PgRow> = query(
            "SELECT * FROM locker_operations WHERE loan_id = $1 AND command_type = 'CHECKOUT'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(loan_id)
        .fetch_optional(&mut *tx)
        .await?;
        return match operation {
            Some(operation) => {
                tx.commit().await?;
                Ok((StatusCode::ACCEPTED, Json(dump_row(&operation))))
            }
            None => Err(ApiError::conflict("Se requiere una autorización aprobada")),
        };
    }

    // status == APPROVED: lock the tool, then enforce one active loan per tool.
    query("SELECT id FROM tools WHERE id = $1 FOR UPDATE")
        .bind(tool_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::conflict("No existe ubicación de origen para la herramienta"))?;
    let active_loan: Option<Uuid> = query_scalar(
        "SELECT id FROM loans WHERE tool_id = $1 AND status IN ('CHECKOUT_PENDING', 'ACTIVE')",
    )
    .bind(tool_id)
    .fetch_optional(&mut *tx)
    .await?;
    if active_loan.is_some() {
        return Err(ApiError::conflict(
            "La herramienta ya tiene un préstamo activo o pendiente",
        ));
    }
    let placement: Option<PgRow> = query(
        "SELECT * FROM tool_placements WHERE tool_id = $1 AND removed_at IS NULL
         ORDER BY placed_at DESC LIMIT 1",
    )
    .bind(tool_id)
    .fetch_optional(&mut *tx)
    .await?;
    let placement = match placement {
        Some(placement) => placement,
        None => {
            return Err(ApiError::conflict(
                "No existe ubicación de origen para la herramienta",
            ))
        }
    };

    let user_id: Uuid = authorization.try_get("user_id")?;
    let loan: PgRow = query(
        "INSERT INTO loans (id, tool_id, user_id, authorization_id, status)
         VALUES ($1, $2, $3, $4, 'CHECKOUT_PENDING') RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(tool_id)
    .bind(user_id)
    .bind(authorization_id)
    .fetch_one(&mut *tx)
    .await?;
    let loan_id: Uuid = loan.try_get("id")?;
    query("UPDATE authorizations SET status = 'CONSUMED' WHERE id = $1")
        .bind(authorization_id)
        .execute(&mut *tx)
        .await?;
    let operation: PgRow = insert_operation(
        &mut tx,
        "CHECKOUT",
        tool_id,
        loan_id,
        placement.try_get("branch_id")?,
        placement.try_get("locker_id")?,
        placement.try_get("compartment_id")?,
    )
    .await?;
    let operation_id: Uuid = operation.try_get("id")?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "CHECKOUT_REQUESTED",
        "LockerOperation",
        &operation_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(dump_row(&operation))))
}

/// POST /loans/{loan_id}/return — same transactional pattern as checkout.
/// The destination compartment must be free and not reserved (audit #8).
pub async fn return_loan(
    State(pool): State<PgPool>,
    Path(loan_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let mut tx = pool.begin().await?;
    let loan: Option<PgRow> = query("SELECT * FROM loans WHERE id = $1 FOR UPDATE")
        .bind(loan_id)
        .fetch_optional(&mut *tx)
        .await?;
    let loan = match loan {
        Some(loan) => loan,
        None => return Err(ApiError::conflict("El préstamo no está activo")),
    };
    let status: String = loan.try_get("status")?;
    let tool_id: Uuid = loan.try_get("tool_id")?;
    if status == "RETURN_PENDING" {
        // Idempotent retry: return the operation already created for this loan.
        let operation: Option<PgRow> = query(
            "SELECT * FROM locker_operations WHERE loan_id = $1 AND command_type = 'RETURN'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(loan_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(operation) = operation {
            tx.commit().await?;
            return Ok((StatusCode::ACCEPTED, Json(dump_row(&operation))));
        }
        return Err(ApiError::conflict("El préstamo no está activo"));
    }
    if status != "ACTIVE" {
        return Err(ApiError::conflict("El préstamo no está activo"));
    }

    let placement: Option<PgRow> =
        query("SELECT * FROM tool_placements WHERE tool_id = $1 ORDER BY placed_at DESC LIMIT 1")
            .bind(tool_id)
            .fetch_optional(&mut *tx)
            .await?;
    let placement = match placement {
        Some(placement) => placement,
        None => {
            return Err(ApiError::conflict(
                "No existe ubicación de origen para la herramienta",
            ))
        }
    };
    let compartment_id: Uuid = placement.try_get("compartment_id")?;
    // Serialize concurrent returns/placements into the same compartment so the
    // occupancy and reservation checks below cannot race each other.
    query("SELECT id FROM compartments WHERE id = $1 FOR UPDATE")
        .bind(compartment_id)
        .fetch_optional(&mut *tx)
        .await?;
    let occupied: Option<Uuid> = query_scalar(
        "SELECT id FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
    )
    .bind(compartment_id)
    .fetch_optional(&mut *tx)
    .await?;
    if occupied.is_some() {
        return Err(ApiError::conflict(
            "El compartimiento de destino está ocupado",
        ));
    }
    let reserved: Option<Uuid> = query_scalar(
        // A pending-ish RETURN reserves the compartment until it reaches a
        // terminal state; UNKNOWN keeps the reservation because the physical
        // outcome is indeterminable (corrección C).
        "SELECT id FROM locker_operations WHERE compartment_id = $1
          AND status IN ('PENDING','PUBLISHING','PUBLISHED','UNKNOWN') AND command_type = 'RETURN'",
    )
    .bind(compartment_id)
    .fetch_optional(&mut *tx)
    .await?;
    if reserved.is_some() {
        return Err(ApiError::conflict(
            "El compartimiento está reservado por una devolución pendiente",
        ));
    }

    query("UPDATE loans SET status = 'RETURN_PENDING' WHERE id = $1")
        .bind(loan_id)
        .execute(&mut *tx)
        .await?;
    let operation: PgRow = insert_operation(
        &mut tx,
        "RETURN",
        tool_id,
        loan_id,
        placement.try_get("branch_id")?,
        placement.try_get("locker_id")?,
        compartment_id,
    )
    .await?;
    let operation_id: Uuid = operation.try_get("id")?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "RETURN_REQUESTED",
        "LockerOperation",
        &operation_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(dump_row(&operation))))
}

/// Event data that does not match the command fails the operation without
/// touching the domain (mirror of Python's confirm mismatch branch). The
/// command may still have been executed by the device, so the loan stays
/// pending and the operator reconciles through `resolve` (uncertain outcome,
/// never an automatic rollback).
pub async fn apply_event_mismatch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation: &PgRow,
) -> Result<(), sqlx::Error> {
    let operation_id: Uuid = operation.try_get("id")?;
    query(
        "UPDATE locker_operations SET status = 'FAILED', confirmed_at = now(),
                failure_reason = 'Command data does not match event'
         WHERE id = $1 AND status IN ('PUBLISHING','PUBLISHED','UNKNOWN')",
    )
    .bind(operation_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// POST /operations/{id}/resolve — administrative reconciliation (corrección C).
/// Accepts an UNKNOWN operation (indeterminable delivery) and a FAILED one that
/// still holds a pending loan (event-time failure: the compartment may have
/// opened, so only the operator's attestation releases its resources).
/// Requires the lab-admin bearer token, a verdict, a reason and evidence;
/// every resolution lands in the audit trail. A late MQTT event after
/// resolution is ignored: the terminal state and the operator's attestation
/// stand, and the event is logged for follow-up.
pub async fn resolve_operation(
    State(pool): State<PgPool>,
    Path(operation_id): Path<Uuid>,
    headers: HeaderMap,
    ValidJson(payload): ValidJson,
) -> Result<Json<Value>, ApiError> {
    // Lab-only guard (audit #1 defers real auth to Phase 5); the token is the
    // one /auth/login issues, so the frontend flow stays coherent.
    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer lab-admin-token");
    if !authorized {
        return Err(ApiError::unauthorized(
            "Se requiere autorización administrativa",
        ));
    }
    let outcome = required_text(&payload, "outcome")?;
    if !matches!(outcome.as_str(), "CONFIRMED" | "FAILED") {
        return Err(ApiError::validation(
            "Campo 'outcome': debe ser CONFIRMED o FAILED",
        ));
    }
    let reason = bounded_text(&payload, "reason", 255)?;
    let evidence = required_text(&payload, "evidence")?;

    let mut tx = pool.begin().await?;
    let operation: Option<PgRow> =
        query("SELECT * FROM locker_operations WHERE id = $1 FOR UPDATE")
            .bind(operation_id)
            .fetch_optional(&mut *tx)
            .await?;
    let operation = match operation {
        Some(operation) => operation,
        None => return Err(ApiError::not_found("La operación no existe")),
    };
    let status: String = operation.try_get("status")?;
    let command_type: String = operation.try_get("command_type")?;
    if !matches!(command_type.as_str(), "CHECKOUT" | "RETURN") {
        return Err(ApiError::conflict("Comando de operación desconocido"));
    }
    if status == "FAILED" {
        // An event-time FAILED (preconditions broke while the compartment
        // opened, or the event data mismatched) deliberately keeps its loan
        // blocked: the physical opening may have executed, so resources are
        // never released automatically. It stays resolvable while the loan is
        // still pending; the operator's attestation is the only release.
        let loan_id: Uuid = operation.try_get("loan_id")?;
        let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
            .bind(loan_id)
            .fetch_one(&mut *tx)
            .await?;
        if !matches!(loan_status.as_str(), "CHECKOUT_PENDING" | "RETURN_PENDING") {
            return Err(ApiError::conflict(
                "La operación fallida ya no retiene recursos del dominio",
            ));
        }
        // Both outcomes attest a physical result the domain could not record
        // (its preconditions already failed), so either way the pending loan
        // rolls back; the terminal FAILED state itself never changes.
        revert_pending_loan(&mut tx, &operation).await?;
    } else if status != "UNKNOWN" {
        return Err(ApiError::conflict(
            "Solo una operación UNKNOWN o FAILED puede resolverse",
        ));
    }
    if status == "UNKNOWN" && outcome == "CONFIRMED" {
        // The operator attests the physical execution with evidence; the same
        // precondition checks as an MQTT event apply (audit #17). If they fail,
        // the operation ends FAILED and the pending loan rolls back below so
        // no tool or compartment stays blocked by a dead operation.
        crate::mqtt::apply_confirmation(&mut tx, &operation).await?;
        let applied: String = query_scalar("SELECT status FROM locker_operations WHERE id = $1")
            .bind(operation_id)
            .fetch_one(&mut *tx)
            .await?;
        if applied == "FAILED" {
            revert_pending_loan(&mut tx, &operation).await?;
        }
    }
    if status != "FAILED" {
        query("UPDATE locker_operations SET status = 'FAILED', confirmed_at = now(), failure_reason = $2
               WHERE id = $1 AND status = 'UNKNOWN'")
            .bind(operation_id)
            .bind(&reason)
            .execute(&mut *tx)
            .await?;
        // The operator attests the physical action did NOT happen, so the
        // in-flight loan may safely roll back and release its resources.
        if outcome == "FAILED" {
            revert_pending_loan(&mut tx, &operation).await?;
        }
    }
    let final_status: String = query_scalar("SELECT status FROM locker_operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&mut *tx)
        .await?;
    let actor = admin_actor(&mut tx).await;
    // The audit records the operator's verdict plus the resulting status: an
    // attested CONFIRMED whose preconditions failed ends as FAILED here.
    crate::audit::audit_with_metadata(
        &mut tx,
        &format!("OPERATION_RESOLVED_{outcome}"),
        "LockerOperation",
        &operation_id.to_string(),
        actor,
        serde_json::json!({
            "outcome": outcome,
            "status": final_status,
            "reason": reason,
            "evidence": evidence,
        }),
    )
    .await?;
    tx.commit().await?;
    let row: PgRow = query("SELECT * FROM locker_operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&pool)
        .await?;
    Ok(Json(dump_row(&row)))
}

/// Rolls back the in-flight loan of a terminal operation: CHECKOUT loans are
/// cancelled (their authorization becomes reusable) and RETURN loans go back
/// to ACTIVE; conditional updates keep unrelated states intact. Callers guard
/// the command type, so the default branch is unreachable in practice.
/// Shared by `resolve_operation` and the MQTT publish-time failure path.
pub async fn revert_pending_loan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation: &PgRow,
) -> Result<(), sqlx::Error> {
    let command_type: String = operation.try_get("command_type")?;
    let loan_id: Uuid = operation.try_get("loan_id")?;
    match command_type.as_str() {
        "CHECKOUT" => {
            query("UPDATE loans SET status = 'CANCELLED' WHERE id = $1 AND status = 'CHECKOUT_PENDING'")
                .bind(loan_id)
                .execute(&mut **tx)
                .await?;
            let authorization_id: Option<Uuid> =
                query_scalar("SELECT authorization_id FROM loans WHERE id = $1")
                    .bind(loan_id)
                    .fetch_optional(&mut **tx)
                    .await?
                    .flatten();
            if let Some(authorization_id) = authorization_id {
                query(
                    "UPDATE authorizations SET status = 'APPROVED'
                           WHERE id = $1 AND status = 'CONSUMED'",
                )
                .bind(authorization_id)
                .execute(&mut **tx)
                .await?;
            }
        }
        "RETURN" => {
            query("UPDATE loans SET status = 'ACTIVE' WHERE id = $1 AND status = 'RETURN_PENDING'")
                .bind(loan_id)
                .execute(&mut **tx)
                .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Required non-empty text field; empty strings fail like missing ones.
fn required_text(payload: &Value, name: &str) -> Result<String, ApiError> {
    bounded_text(payload, name, usize::MAX)
}

/// Required non-empty text field with a character limit matching the
/// PostgreSQL column (`failure_reason` is VARCHAR(255), counted in
/// characters, so is `chars().count`).
fn bounded_text(payload: &Value, name: &str, max: usize) -> Result<String, ApiError> {
    let text = payload
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::validation(&format!("Campo '{name}': se requiere texto")))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ApiError::validation(&format!(
            "Campo '{name}': no puede estar vacío"
        )));
    }
    if trimmed.chars().count() > max {
        return Err(ApiError::validation(&format!(
            "Campo '{name}': excede el máximo de {max} caracteres"
        )));
    }
    Ok(trimmed.to_owned())
}

#[allow(clippy::too_many_arguments)]
async fn insert_operation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command_type: &str,
    tool_id: Uuid,
    loan_id: Uuid,
    branch_id: Uuid,
    locker_id: Uuid,
    compartment_id: Uuid,
) -> Result<PgRow, ApiError> {
    let row: PgRow = query(
        "INSERT INTO locker_operations
             (id, correlation_id, command_type, status, tool_id, loan_id, branch_id, locker_id, compartment_id, origin)
         VALUES ($1, $2, $3, 'PENDING', $4, $5, $6, $7, $8, 'rust') RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4().to_string())
    .bind(command_type)
    .bind(tool_id)
    .bind(loan_id)
    .bind(branch_id)
    .bind(locker_id)
    .bind(compartment_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

pub async fn list_loans(
    State(pool): State<PgPool>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, ApiError> {
    crate::admin::list(&LOANS, pool, params).await
}

pub async fn list_operations(
    State(pool): State<PgPool>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, ApiError> {
    crate::admin::list(&OPERATIONS, pool, params).await
}
