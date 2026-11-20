use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{postgres::PgRow, query, query_scalar, PgPool, Row};
use uuid::Uuid;

use crate::audit::{admin_actor, audit};
use crate::dump::dump_row;
use crate::entities::uuid_field;
use crate::error::ApiError;
use crate::json::ValidJson;

#[derive(Deserialize)]
pub struct PlacementParams {
    pub tool_id: Option<Uuid>,
}

/// GET /placements?tool_id=
pub async fn list_placements(
    State(pool): State<PgPool>,
    Query(params): Query<PlacementParams>,
) -> Result<Json<Value>, ApiError> {
    let rows: Vec<PgRow> = match params.tool_id {
        Some(tool_id) => {
            query("SELECT * FROM tool_placements WHERE tool_id = $1 ORDER BY placed_at DESC")
                .bind(tool_id)
                .fetch_all(&pool)
                .await?
        }
        None => {
            query("SELECT * FROM tool_placements ORDER BY placed_at DESC")
                .fetch_all(&pool)
                .await?
        }
    };
    Ok(Json(Value::Array(rows.iter().map(dump_row).collect())))
}

/// POST /placements — validates the branch→locker→compartment hierarchy (audit #8)
/// and refuses occupied or reserved compartments inside one transaction.
/// The partial unique indexes on tool_placements backstop any race.
pub async fn create_placement(
    State(pool): State<PgPool>,
    ValidJson(payload): ValidJson,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let tool_id = uuid_field(&payload, "tool_id")?;
    let branch_id = uuid_field(&payload, "branch_id")?;
    let locker_id = uuid_field(&payload, "locker_id")?;
    let compartment_id = uuid_field(&payload, "compartment_id")?;

    let mut tx = pool.begin().await?;
    let compartment_locker: Option<Uuid> =
        query_scalar("SELECT locker_id FROM compartments WHERE id = $1")
            .bind(compartment_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(compartment_locker) = compartment_locker else {
        return Err(ApiError::validation("Compartimiento inexistente"));
    };
    if compartment_locker != locker_id {
        return Err(ApiError::conflict(
            "El compartimiento no pertenece al locker indicado",
        ));
    }
    let locker_branch: Option<Uuid> = query_scalar("SELECT branch_id FROM lockers WHERE id = $1")
        .bind(locker_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(locker_branch) = locker_branch else {
        return Err(ApiError::validation("Locker inexistente"));
    };
    if locker_branch != branch_id {
        return Err(ApiError::conflict(
            "El locker no pertenece a la sucursal indicado",
        ));
    }
    for check in [
        (
            "SELECT 1 FROM tools WHERE id = $1",
            tool_id,
            "Herramienta inexistente",
        ),
        (
            "SELECT 1 FROM branches WHERE id = $1",
            branch_id,
            "Sucursal inexistente",
        ),
    ] {
        let found: Option<i32> = query_scalar(check.0)
            .bind(check.1)
            .fetch_optional(&mut *tx)
            .await?;
        if found.is_none() {
            return Err(ApiError::validation(check.2));
        }
    }
    // Serialize against concurrent returns targeting the same compartment.
    query("SELECT id FROM compartments WHERE id = $1 FOR UPDATE")
        .bind(compartment_id)
        .fetch_optional(&mut *tx)
        .await?;
    let tool_active: Option<Uuid> =
        query_scalar("SELECT id FROM tool_placements WHERE tool_id = $1 AND removed_at IS NULL")
            .bind(tool_id)
            .fetch_optional(&mut *tx)
            .await?;
    if tool_active.is_some() {
        return Err(ApiError::conflict(
            "Herramienta o compartimiento ya tiene una ubicación activa",
        ));
    }
    let compartment_active: Option<Uuid> = query_scalar(
        "SELECT id FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
    )
    .bind(compartment_id)
    .fetch_optional(&mut *tx)
    .await?;
    if compartment_active.is_some() {
        return Err(ApiError::conflict(
            "Herramienta o compartimiento ya tiene una ubicación activa",
        ));
    }
    let reserved: Option<Uuid> = query_scalar(
        // Same reservation rule as `return_loan`: a RETURN keeps the
        // compartment reserved until it reaches a terminal, reverted state;
        // UNKNOWN holds it because the physical outcome is indeterminable.
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
    let row: PgRow = query(
        "INSERT INTO tool_placements (id, tool_id, branch_id, locker_id, compartment_id, reason)
         VALUES ($1, $2, $3, $4, $5, 'INITIAL_ASSIGNMENT') RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(tool_id)
    .bind(branch_id)
    .bind(locker_id)
    .bind(compartment_id)
    .fetch_one(&mut *tx)
    .await?;
    let placement_id: Uuid = row.try_get("id")?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "TOOL_PLACED",
        "ToolPlacement",
        &placement_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(dump_row(&row))))
}

/// GET /tools/{tool_id}/history
pub async fn tool_history(
    State(pool): State<PgPool>,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows: Vec<PgRow> =
        query("SELECT * FROM tool_placements WHERE tool_id = $1 ORDER BY placed_at DESC")
            .bind(tool_id)
            .fetch_all(&pool)
            .await?;
    Ok(Json(Value::Array(rows.iter().map(dump_row).collect())))
}

/// GET /lockers/{locker_id}/detail
pub async fn locker_detail(
    State(pool): State<PgPool>,
    Path(locker_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let locker: Option<PgRow> = query("SELECT * FROM lockers WHERE id = $1")
        .bind(locker_id)
        .fetch_optional(&pool)
        .await?;
    let locker = locker.ok_or_else(|| ApiError::not_found("Locker no encontrado"))?;

    let cameras: Vec<PgRow> = query("SELECT * FROM cameras WHERE locker_id = $1")
        .bind(locker_id)
        .fetch_all(&pool)
        .await?;
    let compartments: Vec<PgRow> =
        query("SELECT * FROM compartments WHERE locker_id = $1 ORDER BY position")
            .bind(locker_id)
            .fetch_all(&pool)
            .await?;
    let active_placements: Vec<PgRow> =
        query("SELECT * FROM tool_placements WHERE locker_id = $1 AND removed_at IS NULL")
            .bind(locker_id)
            .fetch_all(&pool)
            .await?;
    let tools: Vec<PgRow> = query("SELECT * FROM tools").fetch_all(&pool).await?;
    let compartment_ids: Vec<Uuid> = compartments
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<sqlx::Result<_>>()?;
    let locks: Vec<PgRow> = if compartment_ids.is_empty() {
        Vec::new()
    } else {
        query("SELECT * FROM locker_locks WHERE compartment_id = ANY($1)")
            .bind(compartment_ids)
            .fetch_all(&pool)
            .await?
    };

    let mut active_by_compartment: HashMap<Uuid, &PgRow> = HashMap::new();
    for placement in &active_placements {
        active_by_compartment.insert(placement.try_get::<Uuid, _>("compartment_id")?, placement);
    }
    let mut tools_by_id: HashMap<Uuid, Value> = HashMap::new();
    for tool in &tools {
        tools_by_id.insert(tool.try_get::<Uuid, _>("id")?, dump_row(tool));
    }
    let mut locks_by_compartment: HashMap<Uuid, Value> = HashMap::new();
    for lock in &locks {
        locks_by_compartment.insert(lock.try_get::<Uuid, _>("compartment_id")?, dump_row(lock));
    }

    let mut compartment_values = Vec::with_capacity(compartments.len());
    for compartment in &compartments {
        let id: Uuid = compartment.try_get("id")?;
        let mut value: Map<String, Value> = dump_row(compartment)
            .as_object()
            .cloned()
            .unwrap_or_default();
        let tool = active_by_compartment
            .get(&id)
            .and_then(|placement| placement.try_get::<Uuid, _>("tool_id").ok())
            .and_then(|tool_id| tools_by_id.get(&tool_id).cloned());
        value.insert("tool".to_owned(), tool.unwrap_or(Value::Null));
        value.insert(
            "lock".to_owned(),
            locks_by_compartment
                .get(&id)
                .cloned()
                .unwrap_or(Value::Null),
        );
        compartment_values.push(Value::Object(value));
    }

    Ok(Json(json!({
        "locker": dump_row(&locker),
        "cameras": cameras.iter().map(dump_row).collect::<Vec<_>>(),
        "compartments": compartment_values,
    })))
}
