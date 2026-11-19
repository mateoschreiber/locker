use std::time::Duration;

use axum::{http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

pub fn router(pool: PgPool, timeout: Duration) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(Health { status: "ok" }) }),
        )
        .route(
            "/health/ready",
            get(move || {
                let pool = pool.clone();
                async move {
                    match tokio::time::timeout(
                        timeout,
                        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool),
                    )
                    .await
                    {
                        Ok(Ok(1)) => (StatusCode::OK, Json(Health { status: "ready" })),
                        _ => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(Health {
                                status: "unavailable",
                            }),
                        ),
                    }
                }
            }),
        )
}
