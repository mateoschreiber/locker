mod admin;
mod audit;
mod auth;
mod config;
mod dashboard;
mod dump;
mod entities;
mod error;
mod health;
mod inventory;
mod json;
pub mod mqtt;
mod operations;
mod presence;
pub mod seed;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use sqlx::PgPool;
use std::sync::atomic::AtomicBool;
use std::{sync::Arc, time::Duration};

pub use config::{MqttSettings, Settings};
pub use presence::{new_store, PresenceStore};

/// Full application router: health at the root, the migrated domain under `/api/v1`.
/// `mqtt_ready` gates `/health/ready` on the MQTT events subscription;
/// `presence` serves the observed locker presence (F4.1-A).
pub fn router(
    pool: PgPool,
    timeout: Duration,
    mqtt_ready: Option<Arc<AtomicBool>>,
    presence: PresenceStore,
) -> Router {
    let mut api = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/me", get(auth::me))
        .route(
            "/placements",
            get(inventory::list_placements).post(inventory::create_placement),
        )
        .route("/tools/{tool_id}/history", get(inventory::tool_history))
        .route("/lockers/{locker_id}/detail", get(inventory::locker_detail))
        .route(
            "/authorizations",
            get(operations::list_authorizations).post(operations::create_authorization),
        )
        .route(
            "/authorizations/{authorization_id}/approve",
            post(operations::approve_authorization),
        )
        .route(
            "/authorizations/{authorization_id}/cancel",
            post(operations::cancel_authorization),
        )
        .route(
            "/authorizations/{authorization_id}/checkout",
            post(operations::checkout),
        )
        .route("/loans", get(operations::list_loans))
        .route("/loans/{loan_id}/return", post(operations::return_loan))
        .route("/operations", get(operations::list_operations))
        .route(
            "/operations/{operation_id}/resolve",
            post(operations::resolve_operation),
        )
        .route("/audit", get(dashboard::list_audit))
        .route("/dashboard", get(dashboard::dashboard))
        .route("/capabilities", get(presence::capabilities));
    // Presence needs (pool, store) state of its own; merged after with_state.
    let presence_api = Router::new()
        .route("/lockers/presence", get(presence::lockers_presence))
        .with_state((pool.clone(), presence));
    for spec in entities::ADMIN {
        let path = format!("/{}", spec.path);
        let entity = Router::new()
            .route(
                "/",
                get(move |State(pool): State<PgPool>, Query(params): Query<admin::ListParams>| async move {
                    admin::list(spec, pool, params).await
                })
                .post(
                    move |State(pool): State<PgPool>, body: crate::json::ValidJson| async move {
                        admin::create(spec, pool, body).await
                    },
                ),
            )
            .route(
                "/{item_id}",
                get(move |State(pool): State<PgPool>, Path(item_id): Path<uuid::Uuid>| async move {
                    admin::get_one(spec, pool, item_id).await
                })
                .patch(
                    move |State(pool): State<PgPool>, Path(item_id): Path<uuid::Uuid>,
                    body: crate::json::ValidJson| async move {
                        admin::update(spec, pool, item_id, body).await
                    },
                ),
            )
            .route(
                "/{item_id}/deactivate",
                post(move |State(pool): State<PgPool>, Path(item_id): Path<uuid::Uuid>| async move {
                    admin::deactivate(spec, pool, item_id).await
                }),
            );
        api = api.nest(path.as_str(), entity);
    }
    health::router(pool.clone(), timeout, mqtt_ready)
        .nest("/api/v1", api.with_state(pool).merge(presence_api))
}
