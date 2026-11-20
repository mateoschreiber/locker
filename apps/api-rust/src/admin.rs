use axum::{http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, query, query_scalar, PgPool, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use crate::audit::{admin_actor, audit};
use crate::dump::dump_row;
use crate::entities::{field_value, Entity, Val};
use crate::error::ApiError;
use crate::json::ValidJson;

#[derive(Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Maximum page size: bounded so a caller can never demand the whole table
/// through the paginated contract (the legacy full-array one still exists).
pub const MAX_PAGE_SIZE: i64 = 200;

/// Validated pagination. `None` keeps the legacy full-array contract;
/// `Some` answers the explicit `{items, total, limit, offset}` envelope.
/// `offset` without `limit` is a 422, never a silently ignored param.
pub struct Page {
    pub limit: i64,
    pub offset: i64,
}

pub fn page(params: &ListParams) -> Result<Option<Page>, ApiError> {
    let Some(limit) = params.limit else {
        if params.offset.is_some() {
            return Err(ApiError::validation("Campo 'offset': requiere limit"));
        }
        return Ok(None);
    };
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(ApiError::validation(&format!(
            "Campo 'limit': entre 1 y {}",
            MAX_PAGE_SIZE
        )));
    }
    let offset = params.offset.unwrap_or(0);
    if offset < 0 {
        return Err(ApiError::validation(
            "Campo 'offset': debe ser mayor o igual a 0",
        ));
    }
    Ok(Some(Page { limit, offset }))
}

/// Shared WHERE construction so the page query and its COUNT always filter
/// identically. Returns nothing: `has_where` is recomputed per builder.
fn push_filters(builder: &mut sqlx::QueryBuilder<Postgres>, spec: &Entity, params: &ListParams) {
    let mut has_where = false;
    if spec.has_status {
        if let Some(status) = &params.status {
            builder.push(" WHERE status = ").push_bind(status.clone());
            has_where = true;
        }
    }
    if !spec.searchable.is_empty() {
        if let Some(q) = &params.q {
            builder.push(if has_where { " AND (" } else { " WHERE (" });
            for (index, field) in spec.searchable.iter().enumerate() {
                if index > 0 {
                    builder.push(" OR ");
                }
                builder
                    .push(*field)
                    .push(" ILIKE ")
                    .push_bind(format!("%{q}%"));
            }
            builder.push(")");
        }
    }
}

/// GET /{entity}?q=&status=&limit=&offset= — ordered by created_at DESC with
/// an id tiebreak (rows seeded in one transaction share created_at, so the
/// old order was nondeterministic). Without `limit` the response stays the
/// legacy full array (Python parity consumers); with it, the paginated
/// envelope. The COUNT runs the same WHERE as the page.
pub async fn list(
    spec: &'static Entity,
    pool: PgPool,
    params: ListParams,
) -> Result<Json<Value>, ApiError> {
    let page = page(&params)?;
    let mut builder = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    builder.push(spec.table);
    push_filters(&mut builder, spec, &params);
    builder.push(" ORDER BY created_at DESC, id DESC");
    match page {
        None => {
            let rows: Vec<PgRow> = builder.build().fetch_all(&pool).await?;
            Ok(Json(Value::Array(rows.iter().map(dump_row).collect())))
        }
        Some(page) => {
            builder.push(" LIMIT ").push_bind(page.limit);
            builder.push(" OFFSET ").push_bind(page.offset);
            let rows: Vec<PgRow> = builder.build().fetch_all(&pool).await?;
            let mut count_builder = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM ");
            count_builder.push(spec.table);
            push_filters(&mut count_builder, spec, &params);
            // shortcut: one COUNT per paginated poll; fine at lab scale,
            // revisit with an index-free keyset cursor if growth measurements demand it
            let total: i64 = count_builder.build().fetch_one(&pool).await?.try_get(0)?;
            Ok(Json(json!({
                "items": rows.iter().map(dump_row).collect::<Vec<_>>(),
                "total": total,
                "limit": page.limit,
                "offset": page.offset,
            })))
        }
    }
}

/// Validates a manually written status against the entity's allowed list
/// (audit #19): unknown or domain-only states are a 422 contract error.
fn check_status(spec: &Entity, value: &Val) -> Result<(), ApiError> {
    let Val::Text(value) = value else {
        return Ok(());
    };
    if !spec.allowed_statuses.is_empty() && !spec.allowed_statuses.contains(&value.as_str()) {
        return Err(ApiError::validation(&format!(
            "Campo 'status': debe ser uno de {}",
            spec.allowed_statuses.join(", ")
        )));
    }
    Ok(())
}

/// Domain guards for a manual status write (PATCH or deactivate, audit #19):
/// a tool with a pending/active loan and a compartment holding an active
/// placement or a pending-return reservation keep their state until the
/// domain releases it. Row locks (`FOR UPDATE`) serialize against the
/// checkout/return/confirmation flows, which take the same locks.
async fn guard_status_change(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    spec: &'static Entity,
    item_id: Uuid,
) -> Result<(), ApiError> {
    match spec.path {
        "tools" => {
            query("SELECT id FROM tools WHERE id = $1 FOR UPDATE")
                .bind(item_id)
                .fetch_optional(&mut **tx)
                .await?;
            let active_loan: Option<Uuid> = query_scalar(
                "SELECT id FROM loans WHERE tool_id = $1
                  AND status IN ('CHECKOUT_PENDING','ACTIVE','RETURN_PENDING')",
            )
            .bind(item_id)
            .fetch_optional(&mut **tx)
            .await?;
            if active_loan.is_some() {
                return Err(ApiError::conflict(
                    "La herramienta tiene un préstamo activo o pendiente",
                ));
            }
        }
        "compartments" => {
            query("SELECT id FROM compartments WHERE id = $1 FOR UPDATE")
                .bind(item_id)
                .fetch_optional(&mut **tx)
                .await?;
            let blocked: Option<Uuid> = query_scalar(
                "SELECT id FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
            )
            .bind(item_id)
            .fetch_optional(&mut **tx)
            .await?;
            let blocked = match blocked {
                Some(id) => Some(id),
                None => {
                    query_scalar(
                        "SELECT id FROM locker_operations WHERE compartment_id = $1
                       AND status IN ('PENDING','PUBLISHING','PUBLISHED','UNKNOWN')
                       AND command_type = 'RETURN'",
                    )
                    .bind(item_id)
                    .fetch_optional(&mut **tx)
                    .await?
                }
            };
            if blocked.is_some() {
                return Err(ApiError::conflict(
                    "El compartimiento tiene una ubicación activa o una devolución pendiente",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// GET /{entity}/{item_id}
pub async fn get_one(
    spec: &'static Entity,
    pool: PgPool,
    item_id: Uuid,
) -> Result<Json<Value>, ApiError> {
    let row: Option<PgRow> = query(&select_by_id(spec))
        .bind(item_id)
        .fetch_optional(&pool)
        .await?;
    let row = row.ok_or_else(|| ApiError::not_found("Registro no encontrado"))?;
    Ok(Json(dump_row(&row)))
}

/// POST /{entity} — validated payload; unknown fields are ignored.
pub async fn create(
    spec: &'static Entity,
    pool: PgPool,
    ValidJson(payload): ValidJson,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if !payload.is_object() {
        return Err(ApiError::validation("Se requiere un objeto JSON"));
    }
    let mut columns: Vec<&'static str> = vec!["id"];
    let mut values: Vec<Val> = vec![Val::Uuid(Uuid::new_v4())];
    for field in spec.fields {
        match field_value(field, &payload)? {
            Some(value) => {
                if field.name == "status" {
                    check_status(spec, &value)?;
                }
                columns.push(field.name);
                values.push(value);
            }
            None => {
                if field.required {
                    return Err(ApiError::validation(&format!(
                        "Campo requerido: '{}'",
                        field.name
                    )));
                }
                if let Some(default) = field.default {
                    columns.push(field.name);
                    values.push(Val::Text(default.to_owned()));
                }
            }
        }
    }
    let mut tx = pool.begin().await?;
    let mut builder = QueryBuilder::<Postgres>::new("INSERT INTO ");
    builder
        .push(spec.table)
        .push(" (")
        .push(columns.join(", "))
        .push(") VALUES (");
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            builder.push(", ");
        }
        match value {
            Val::Text(value) => {
                builder.push_bind(value);
            }
            Val::Uuid(value) => {
                builder.push_bind(value);
            }
            Val::Int(value) => {
                builder.push_bind(value);
            }
        }
    }
    builder.push(") RETURNING *");
    let row: PgRow = builder.build().fetch_one(&mut *tx).await?;
    let item_id: Uuid = row.try_get("id")?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        &format!("{}_CREATED", spec.action),
        spec.class_name,
        &item_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(dump_row(&row))))
}

/// PATCH /{entity}/{item_id} — only editable fields; unknown fields ignored (Python parity).
pub async fn update(
    spec: &'static Entity,
    pool: PgPool,
    item_id: Uuid,
    ValidJson(payload): ValidJson,
) -> Result<Json<Value>, ApiError> {
    if !payload.is_object() {
        return Err(ApiError::validation("Se requiere un objeto JSON"));
    }
    let mut updates: Vec<(&'static str, Val)> = Vec::new();
    for field in spec.fields {
        if field.editable {
            if let Some(value) = field_value(field, &payload)? {
                if field.name == "status" {
                    check_status(spec, &value)?;
                }
                updates.push((field.name, value));
            }
        }
    }
    let mut tx = pool.begin().await?;
    if updates.iter().any(|(column, _)| *column == "status") {
        // 404 before the guard, keeping the not-found contract; the guard
        // then refuses writes that contradict active domain state.
        fetch_row(&mut tx, spec, item_id).await?;
        guard_status_change(&mut tx, spec, item_id).await?;
    }
    let row: PgRow = if updates.is_empty() {
        fetch_row(&mut tx, spec, item_id).await?
    } else {
        let mut builder = QueryBuilder::<Postgres>::new("UPDATE ");
        builder.push(spec.table).push(" SET ");
        for (index, (column, value)) in updates.iter().enumerate() {
            if index > 0 {
                builder.push(", ");
            }
            builder.push(*column).push(" = ");
            match value {
                Val::Text(value) => {
                    builder.push_bind(value.clone());
                }
                Val::Uuid(value) => {
                    builder.push_bind(*value);
                }
                Val::Int(value) => {
                    builder.push_bind(*value);
                }
            }
        }
        builder
            .push(" WHERE id = ")
            .push_bind(item_id)
            .push(" RETURNING *");
        builder
            .build()
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::not_found("Registro no encontrado"))?
    };
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        &format!("{}_UPDATED", spec.action),
        spec.class_name,
        &item_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(dump_row(&row)))
}

/// POST /{entity}/{item_id}/deactivate
pub async fn deactivate(
    spec: &'static Entity,
    pool: PgPool,
    item_id: Uuid,
) -> Result<Json<Value>, ApiError> {
    let mut tx = pool.begin().await?;
    // Python checks existence first (404) and then deactivation support (409).
    fetch_row(&mut tx, spec, item_id).await?;
    if !spec.has_status {
        return Err(ApiError::conflict("La entidad no admite desactivación"));
    }
    // Deactivation is a status write: the same domain guards as PATCH apply,
    // so `deactivate` cannot bypass them (audit #19).
    guard_status_change(&mut tx, spec, item_id).await?;
    let row: PgRow = query(&format!(
        "UPDATE {} SET status = 'INACTIVE' WHERE id = $1 RETURNING *",
        spec.table
    ))
    .bind(item_id)
    .fetch_one(&mut *tx)
    .await?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        &format!("{}_DEACTIVATED", spec.action),
        spec.class_name,
        &item_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(dump_row(&row)))
}

fn select_by_id(spec: &Entity) -> String {
    format!("SELECT * FROM {} WHERE id = $1", spec.table)
}

async fn fetch_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    spec: &'static Entity,
    item_id: Uuid,
) -> Result<PgRow, ApiError> {
    query(&select_by_id(spec))
        .bind(item_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::not_found("Registro no encontrado"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(limit: Option<i64>, offset: Option<i64>) -> ListParams {
        ListParams {
            q: None,
            status: None,
            limit,
            offset,
        }
    }

    #[test]
    fn page_contract_validation() {
        // Without limit the legacy full-array contract stays (None).
        assert!(matches!(page(&params(None, None)), Ok(None)));
        // offset without limit is an explicit 422, never a silent ignore.
        assert!(page(&params(None, Some(5))).is_err());
        let Ok(Some(paged)) = page(&params(Some(50), None)) else {
            panic!("valid limit rejected");
        };
        assert_eq!((paged.limit, paged.offset), (50, 0));
        let Ok(Some(paged)) = page(&params(Some(200), Some(30))) else {
            panic!("max limit with offset rejected");
        };
        assert_eq!((paged.limit, paged.offset), (200, 30));
        for bad in [0, -1, 201, i64::MAX] {
            assert!(
                page(&params(Some(bad), None)).is_err(),
                "limit {bad} accepted"
            );
        }
        assert!(page(&params(Some(10), Some(-1))).is_err());
    }
}
