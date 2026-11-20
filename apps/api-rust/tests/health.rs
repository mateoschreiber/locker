use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use locker_api_rust::{new_store, router, MqttSettings, Settings};
use tower::ServiceExt;

fn settings(url: &str) -> Settings {
    Settings {
        database_url: url.to_owned(),
        bind: "127.0.0.1:0".parse().unwrap(),
        max_connections: 2,
        timeout: Duration::from_secs(1),
        mqtt: MqttSettings {
            enabled: false,
            host: "127.0.0.1".to_owned(),
            port: 1883,
            client_id: "test".to_owned(),
            confirm_timeout: Duration::from_secs(60),
            stale_seconds: 90,
        },
    }
}

#[tokio::test]
async fn live_and_unavailable_database() {
    let config = settings("postgres://test:test@127.0.0.1:1/test");
    let app = router(
        config.pool().unwrap(),
        config.timeout,
        None,
        new_store(Duration::from_secs(90)),
    );
    let live = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(live.status(), StatusCode::OK);
    let ready = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn ready_when_test_database_is_available() {
    let url = std::env::var("LOCKER_TEST_DATABASE_URL").expect("LOCKER_TEST_DATABASE_URL required");
    let config = settings(&url);
    let app = router(
        config.pool().unwrap(),
        config.timeout,
        None,
        new_store(Duration::from_secs(90)),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
