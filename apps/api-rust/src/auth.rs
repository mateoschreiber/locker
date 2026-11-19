use axum::{extract::State, Json};
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, query, PgPool, Row};
use uuid::Uuid;

use crate::audit::audit;
use crate::dump::dump_row;
use crate::error::ApiError;
use crate::json::ValidJson;

/// POST /auth/login — lab credentials in clear text are an accepted P0 risk
/// until Phase 5; the contract and `admin`/`admin` access stay unchanged.
pub async fn login(
    State(pool): State<PgPool>,
    ValidJson(payload): ValidJson,
) -> Result<Json<Value>, ApiError> {
    let username = payload
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = payload
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut tx = pool.begin().await?;
    let row: Option<PgRow> = query("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(ApiError::unauthorized("Credenciales inválidas"));
    };
    let stored: String = row.try_get("password")?;
    if stored != password {
        return Err(ApiError::unauthorized("Credenciales inválidas"));
    }
    let user_id: Uuid = row.try_get("id")?;
    audit(
        &mut tx,
        "LOGIN",
        "User",
        &user_id.to_string(),
        Some(user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({
        "access_token": "lab-admin-token",
        "token_type": "bearer",
        "user": dump_row(&row),
    })))
}

/// GET /auth/me — hardcoded admin, matching the Python lab contract.
pub async fn me(State(pool): State<PgPool>) -> Result<Json<Value>, ApiError> {
    let row: Option<PgRow> = query("SELECT * FROM users WHERE username = 'admin'")
        .fetch_optional(&pool)
        .await?;
    let row = row.ok_or_else(|| ApiError::not_found("Administrador no encontrado"))?;
    Ok(Json(dump_row(&row)))
}
