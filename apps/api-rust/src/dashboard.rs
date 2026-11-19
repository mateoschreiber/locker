use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, query, PgPool};

use crate::admin::ListParams;
use crate::dump::dump_row;
use crate::entities::AUDIT;
use crate::error::ApiError;

/// GET /audit — q is accepted for Python parity but ignored: the Python
/// `list_items` only searches name/code/asset_code/username/display_name,
/// none of which exist on audit_events.
pub async fn list_audit(
    State(pool): State<PgPool>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, ApiError> {
    crate::admin::list(&AUDIT, pool, params).await
}

/// GET /dashboard
pub async fn dashboard(State(pool): State<PgPool>) -> Result<Json<Value>, ApiError> {
    let lockers = count(&pool, "SELECT COUNT(*) FROM lockers").await?;
    let tools_available = count(
        &pool,
        "SELECT COUNT(*) FROM tools WHERE status = 'AVAILABLE'",
    )
    .await?;
    let tools_on_loan = count(&pool, "SELECT COUNT(*) FROM tools WHERE status = 'ON_LOAN'").await?;
    let pending_loans = count(
        &pool,
        "SELECT COUNT(*) FROM loans WHERE status IN ('CHECKOUT_PENDING', 'RETURN_PENDING')",
    )
    .await?;
    let recent: Vec<PgRow> = query("SELECT * FROM audit_events ORDER BY created_at DESC LIMIT 10")
        .fetch_all(&pool)
        .await?;
    Ok(Json(json!({
        "lockers": lockers,
        "tools_available": tools_available,
        "tools_on_loan": tools_on_loan,
        "pending_loans": pending_loans,
        "recent_activity": recent.iter().map(dump_row).collect::<Vec<_>>(),
    })))
}

async fn count(pool: &PgPool, sql: &str) -> Result<i64, ApiError> {
    let value: i64 = sqlx::query_scalar(sql).fetch_one(pool).await?;
    Ok(value)
}
