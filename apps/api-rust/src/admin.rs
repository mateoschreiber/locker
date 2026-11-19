use axum::{http::StatusCode, Json};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{postgres::PgRow, query, PgPool, Postgres, QueryBuilder, Row};
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
}

/// GET /{entity}?q=&status= — ordered by created_at DESC, like Python's `list_items`.
pub async fn list(
    spec: &'static Entity,
    pool: PgPool,
    params: ListParams,
) -> Result<Json<Value>, ApiError> {
    let mut builder = QueryBuilder::<Postgres>::new("SELECT * FROM ");
    builder.push(spec.table);
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
    builder.push(" ORDER BY created_at DESC");
    let rows: Vec<PgRow> = builder.build().fetch_all(&pool).await?;
    Ok(Json(Value::Array(rows.iter().map(dump_row).collect())))
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
                updates.push((field.name, value));
            }
        }
    }
    let mut tx = pool.begin().await?;
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
