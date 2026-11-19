use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

/// `mqtt_ready` gates readiness on the events subscription (Fase 3), mirroring
/// the Python bridge; `None` keeps DB-only readiness for non-MQTT harnesses.
pub fn router(pool: PgPool, timeout: Duration, mqtt_ready: Option<Arc<AtomicBool>>) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(Health { status: "ok" }) }),
        )
        .route(
            "/health/ready",
            get(move || {
                let pool = pool.clone();
                let mqtt_ready = mqtt_ready.clone();
                async move {
                    let mqtt_ok = mqtt_ready
                        .map(|flag| flag.load(Ordering::Relaxed))
                        .unwrap_or(true);
                    match tokio::time::timeout(
                        timeout,
                        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool),
                    )
                    .await
                    {
                        Ok(Ok(1)) if mqtt_ok => (StatusCode::OK, Json(Health { status: "ready" })),
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
