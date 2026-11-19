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
mod operations;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use sqlx::PgPool;
use std::time::Duration;

pub use config::Settings;

/// Full application router: health at the root, the migrated domain under `/api/v1`.
pub fn router(pool: PgPool, timeout: Duration) -> Router {
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
        .route("/audit", get(dashboard::list_audit))
        .route("/dashboard", get(dashboard::dashboard));
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
    health::router(pool.clone(), timeout).nest("/api/v1", api.with_state(pool))
}
