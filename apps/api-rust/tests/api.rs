//! Integration tests against isolated PostgreSQL (`LOCKER_TEST_DATABASE_URL`).
//! Like `health.rs`, they only run under `cargo test -- --ignored`.
//! All DB tests share one database, so a static mutex serializes them.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use locker_api_rust::router;
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, query, query_scalar, PgPool};
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Lab {
    admin: Uuid,
    branch: Uuid,
    branch2: Uuid,
    locker: Uuid,
    locker2: Uuid,
    locker3: Uuid,
    compartment: Uuid,
    compartment2: Uuid,
    compartment3: Uuid,
    tool: Uuid,
    tool2: Uuid,
}

async fn setup() -> (PgPool, Router, tokio::sync::MutexGuard<'static, ()>) {
    // The guard serializes the whole test body: one shared test database.
    let guard = LOCK.lock().await;
    let url = std::env::var("LOCKER_TEST_DATABASE_URL").expect("LOCKER_TEST_DATABASE_URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .expect("connect isolated test database");
    sqlx::raw_sql(include_str!("schema.sql"))
        .execute(&pool)
        .await
        .expect("recreate schema");
    (pool.clone(), router(pool, Duration::from_secs(5)), guard)
}

async fn seed(pool: &PgPool) -> Lab {
    let lab = Lab {
        admin: Uuid::new_v4(),
        branch: Uuid::new_v4(),
        branch2: Uuid::new_v4(),
        locker: Uuid::new_v4(),
        locker2: Uuid::new_v4(),
        locker3: Uuid::new_v4(),
        compartment: Uuid::new_v4(),
        compartment2: Uuid::new_v4(),
        compartment3: Uuid::new_v4(),
        tool: Uuid::new_v4(),
        tool2: Uuid::new_v4(),
    };
    query("INSERT INTO users (id, username, display_name, password, status) VALUES ($1, 'admin', 'Administrador', 'admin', 'ACTIVE')")
        .bind(lab.admin)
        .execute(pool)
        .await
        .unwrap();
    for (id, code) in [(lab.branch, "LAB"), (lab.branch2, "OTRA")] {
        query(
            "INSERT INTO branches (id, code, name, status) VALUES ($1, $2, 'Sucursal', 'ACTIVE')",
        )
        .bind(id)
        .bind(code)
        .execute(pool)
        .await
        .unwrap();
    }
    for (id, branch, code) in [
        (lab.locker, lab.branch, "L1"),
        (lab.locker2, lab.branch, "L2"),
        (lab.locker3, lab.branch2, "L1"),
    ] {
        query("INSERT INTO lockers (id, branch_id, code, name, status) VALUES ($1, $2, $3, 'Locker', 'ONLINE')")
            .bind(id)
            .bind(branch)
            .bind(code)
            .execute(pool)
            .await
            .unwrap();
    }
    for (id, locker, position) in [
        (lab.compartment, lab.locker, 1),
        (lab.compartment2, lab.locker2, 1),
        (lab.compartment3, lab.locker3, 1),
    ] {
        query("INSERT INTO compartments (id, locker_id, code, name, position, status) VALUES ($1, $2, 'C', 'Compartimiento', $3, 'ACTIVE')")
            .bind(id)
            .bind(locker)
            .bind(position)
            .execute(pool)
            .await
            .unwrap();
    }
    for (id, code) in [(lab.tool, "TOOL-001"), (lab.tool2, "TOOL-002")] {
        query("INSERT INTO tools (id, asset_code, name, status) VALUES ($1, $2, 'Herramienta', 'AVAILABLE')")
            .bind(id)
            .bind(code)
            .execute(pool)
            .await
            .unwrap();
    }
    lab
}

async fn place(pool: &PgPool, tool: Uuid, branch: Uuid, locker: Uuid, compartment: Uuid) {
    query("INSERT INTO tool_placements (id, tool_id, branch_id, locker_id, compartment_id, reason) VALUES ($1, $2, $3, $4, $5, 'INITIAL_ASSIGNMENT')")
        .bind(Uuid::new_v4())
        .bind(tool)
        .bind(branch)
        .bind(locker)
        .bind(compartment)
        .execute(pool)
        .await
        .unwrap();
}

async fn create_authorization(app: &Router, tool: Uuid, user: Uuid, branch: Uuid) -> Uuid {
    let (status, value) = post(
        app,
        "/api/v1/authorizations",
        json!({ "tool_id": tool, "user_id": user, "branch_id": branch }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}

async fn request(app: Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("valid JSON response")
    };
    (status, value)
}

async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request(app.clone(), "POST", uri, Some(body)).await
}

async fn action(app: &Router, uri: &str) -> (StatusCode, Value) {
    request(app.clone(), "POST", uri, None).await
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    request(app.clone(), "GET", uri, None).await
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    let value: i64 = query_scalar(sql).fetch_one(pool).await.unwrap();
    value
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn routes_smoke_and_login() {
    let (pool, app, _guard) = setup().await;
    for path in [
        "/api/v1/branches",
        "/api/v1/roles",
        "/api/v1/users",
        "/api/v1/memberships",
        "/api/v1/lockers",
        "/api/v1/compartments",
        "/api/v1/locks",
        "/api/v1/cameras",
        "/api/v1/tools",
        "/api/v1/placements",
        "/api/v1/authorizations",
        "/api/v1/loans",
        "/api/v1/operations",
        "/api/v1/audit",
    ] {
        let (status, value) = get(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(value.is_array(), "{path}: expected array");
    }
    let (status, value) = get(&app, "/api/v1/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    for key in [
        "lockers",
        "tools_available",
        "tools_on_loan",
        "pending_loans",
        "recent_activity",
    ] {
        assert!(value.get(key).is_some(), "dashboard key {key}");
    }
    let (status, _) = get(&app, "/health/live").await;
    assert_eq!(status, StatusCode::OK);

    let (status, value) = post(
        &app,
        "/api/v1/auth/login",
        json!({ "username": "admin", "password": "wrong" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["detail"], "Credenciales inválidas");

    let lab = seed(&pool).await;
    let (status, value) = get(&app, "/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["username"], "admin");
    assert!(
        value.get("password").is_none(),
        "password must never be serialized"
    );

    let (status, value) = post(
        &app,
        "/api/v1/auth/login",
        json!({ "username": "admin", "password": "admin" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["access_token"], "lab-admin-token");
    assert_eq!(value["token_type"], "bearer");
    assert_eq!(value["user"]["username"], "admin");
    assert_eq!(value["user"]["id"], lab.admin.to_string());

    let logins: i64 = count(
        &pool,
        "SELECT COUNT(*) FROM audit_events WHERE action = 'LOGIN'",
    )
    .await;
    // Only the successful login is audited, matching Python.
    assert!(logins >= 1, "login audit rows missing");
    let actor: Option<Uuid> =
        query_scalar("SELECT actor_id FROM audit_events WHERE action = 'LOGIN' LIMIT 1")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .flatten();
    assert_eq!(
        actor,
        Some(lab.admin),
        "LOGIN audit must attribute the actor"
    );
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn admin_crud_validates_contracts() {
    let (pool, app, _guard) = setup().await;
    seed(&pool).await;

    let (status, value) = post(
        &app,
        "/api/v1/branches",
        json!({ "code": "B1", "name": "Sucursal" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["status"], "ACTIVE", "status default missing");
    let branch_id = value["id"].as_str().unwrap().to_owned();

    let (status, value) = post(
        &app,
        "/api/v1/branches",
        json!({ "code": "B1", "name": "Dup" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    let (status, value) = post(&app, "/api/v1/branches", json!({ "code": "B2" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    let (status, value) = post(
        &app,
        "/api/v1/branches",
        json!({ "code": "B3", "name": 42 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    let (status, value) = post(&app, "/api/v1/branches", json!("no-object")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");

    let (status, value) = get(&app, &format!("/api/v1/branches/{branch_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["code"], "B1");
    let missing = Uuid::new_v4();
    let (status, _) = get(&app, &format!("/api/v1/branches/{missing}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, value) = request(
        app.clone(),
        "PATCH",
        &format!("/api/v1/branches/{branch_id}"),
        Some(json!({ "status": "MAINTENANCE" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "MAINTENANCE");
    let (status, value) = request(
        app.clone(),
        "PATCH",
        &format!("/api/v1/branches/{branch_id}"),
        Some(json!({ "unknown": "ignored" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["code"], "B1");

    let (status, value) = get(&app, "/api/v1/branches?q=B1&status=MAINTENANCE").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value.as_array().unwrap().len(),
        1,
        "q/status filter broken: {value}"
    );
    let (status, value) = get(&app, "/api/v1/branches?q=zzz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value.as_array().unwrap().len(), 0);

    let (status, value) = action(&app, &format!("/api/v1/branches/{branch_id}/deactivate")).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "INACTIVE");
    let (status, value) = action(&app, &format!("/api/v1/branches/{branch_id}/deactivate")).await;
    assert_eq!(status, StatusCode::OK, "{value}");

    // roles have no status column: deactivation must be refused (Python parity).
    let role_id = Uuid::new_v4();
    query("INSERT INTO roles (id, code, name) VALUES ($1, 'ADMIN', 'Admin')")
        .bind(role_id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, value) = action(&app, &format!("/api/v1/roles/{role_id}/deactivate")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(value["detail"], "La entidad no admite desactivación");

    // users: password stored with the lab default, never serialized back.
    let (status, value) = post(
        &app,
        "/api/v1/users",
        json!({ "username": "mechanic", "display_name": "Mecánico" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert!(
        value.get("password").is_none(),
        "password must never be serialized"
    );
    let stored: Option<String> =
        query_scalar("SELECT password FROM users WHERE username = 'mechanic'")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .flatten();
    assert_eq!(
        stored.as_deref(),
        Some("admin"),
        "lab password default missing"
    );

    // memberships with an unknown role: FK violation maps to 422.
    let (status, value) = post(&app, "/api/v1/memberships", json!({ "user_id": lab_user(&pool).await, "branch_id": branch_id, "role_id": Uuid::new_v4() })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    assert_eq!(value["detail"], "Referencia inexistente");

    // compartments validate position.
    let locker_id: Uuid = query_scalar("SELECT id FROM lockers LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, value) = post(
        &app,
        "/api/v1/compartments",
        json!({ "locker_id": locker_id, "code": "C99", "name": "C", "position": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
}

async fn lab_user(pool: &PgPool) -> Uuid {
    query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn placements_validate_hierarchy_and_occupancy() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;

    // Wrong hierarchy: compartment belongs to locker2 but payload claims locker.
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": lab.tool, "branch_id": lab.branch, "locker_id": lab.locker, "compartment_id": lab.compartment2 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(
        value["detail"],
        "El compartimiento no pertenece al locker indicado"
    );

    // Wrong branch for the locker.
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": lab.tool, "branch_id": lab.branch, "locker_id": lab.locker3, "compartment_id": lab.compartment3 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(
        value["detail"],
        "El locker no pertenece a la sucursal indicado"
    );

    // Unknown references are validation errors.
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": Uuid::new_v4(), "branch_id": lab.branch, "locker_id": lab.locker, "compartment_id": lab.compartment })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");

    // Valid placement.
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": lab.tool, "branch_id": lab.branch, "locker_id": lab.locker, "compartment_id": lab.compartment })).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["reason"], "INITIAL_ASSIGNMENT");

    // Same tool again, and occupied compartment.
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": lab.tool, "branch_id": lab.branch, "locker_id": lab.locker, "compartment_id": lab.compartment })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(
        value["detail"],
        "Herramienta o compartimiento ya tiene una ubicación activa"
    );
    let (status, value) = post(&app, "/api/v1/placements", json!({ "tool_id": lab.tool2, "branch_id": lab.branch, "locker_id": lab.locker, "compartment_id": lab.compartment })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");

    let (status, value) = get(&app, &format!("/api/v1/tools/{}/history", lab.tool)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value.as_array().unwrap().len(), 1);
    let (status, value) = get(&app, &format!("/api/v1/placements?tool_id={}", lab.tool)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value.as_array().unwrap().len(), 1);

    let (status, value) = get(&app, &format!("/api/v1/lockers/{}/detail", lab.locker)).await;
    assert_eq!(status, StatusCode::OK);
    let compartments = value["compartments"].as_array().unwrap();
    assert_eq!(compartments.len(), 1);
    assert_eq!(compartments[0]["tool"]["asset_code"], "TOOL-001");
    let missing = Uuid::new_v4();
    let (status, _) = get(&app, &format!("/api/v1/lockers/{missing}/detail")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Two concurrent placements of the same tool: exactly one may win
    // (the partial unique index is the backstop for the race).
    let tool3 = Uuid::new_v4();
    query("INSERT INTO tools (id, asset_code, name, status) VALUES ($1, 'TOOL-003', 'T', 'AVAILABLE')")
        .bind(tool3)
        .execute(&pool)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let app = app.clone();
        let body = json!({ "tool_id": tool3, "branch_id": lab.branch, "locker_id": lab.locker2, "compartment_id": lab.compartment2 });
        tasks.push(tokio::spawn(async move {
            post(&app, "/api/v1/placements", body).await
        }));
    }
    let mut created = 0;
    for task in tasks {
        let (status, value) = task.await.unwrap();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::CONFLICT,
            "unexpected status {status}: {value}"
        );
        if status == StatusCode::CREATED {
            created += 1;
        }
    }
    assert_eq!(created, 1, "duplicate active placement allowed");
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn authorization_transitions_and_idempotent_checkout() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    let auth = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    let (status, _) = action(&app, &format!("/api/v1/authorizations/{auth}/checkout")).await;
    assert_eq!(status, StatusCode::CONFLICT, "checkout requires APPROVED");

    let (status, value) = action(&app, &format!("/api/v1/authorizations/{auth}/approve")).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "APPROVED");
    assert!(!value["approved_at"].is_null());
    let (status, _) = action(&app, &format!("/api/v1/authorizations/{auth}/approve")).await;
    assert_eq!(status, StatusCode::CONFLICT, "double approve must fail");

    // Cancel path on a second authorization.
    let auth_cancel = create_authorization(&app, lab.tool2, lab.admin, lab.branch).await;
    let (status, value) = action(
        &app,
        &format!("/api/v1/authorizations/{auth_cancel}/cancel"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "CANCELLED");
    let (status, _) = action(
        &app,
        &format!("/api/v1/authorizations/{auth_cancel}/cancel"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = action(
        &app,
        &format!("/api/v1/authorizations/{auth_cancel}/approve"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "approve after cancel must fail"
    );

    let (status, operation) =
        action(&app, &format!("/api/v1/authorizations/{auth}/checkout")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{operation}");
    assert_eq!(operation["command_type"], "CHECKOUT");
    assert_eq!(
        operation["status"], "PENDING",
        "no physical success without device confirmation"
    );
    assert!(!operation["correlation_id"].is_null());
    let loan_id = operation["loan_id"].as_str().unwrap().to_owned();
    let operation_id = operation["id"].as_str().unwrap().to_owned();

    let loans: i64 = count(&pool, "SELECT COUNT(*) FROM loans").await;
    assert_eq!(loans, 1);
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE authorization_id = $1")
        .bind(auth)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loan_status, "CHECKOUT_PENDING");
    let auth_status: String = query_scalar("SELECT status FROM authorizations WHERE id = $1")
        .bind(auth)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(auth_status, "CONSUMED");

    // Idempotent retry: same authorization returns the same operation.
    let (status, retry) = action(&app, &format!("/api/v1/authorizations/{auth}/checkout")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{retry}");
    assert_eq!(
        retry["id"].as_str().unwrap(),
        operation_id,
        "retry must not duplicate the operation"
    );
    let loans: i64 = count(&pool, "SELECT COUNT(*) FROM loans").await;
    assert_eq!(loans, 1, "retry must not duplicate loans");

    // A second approved authorization for the same tool must be refused (#7).
    let auth2 = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(&app, &format!("/api/v1/authorizations/{auth2}/approve")).await;
    let (status, value) = action(&app, &format!("/api/v1/authorizations/{auth2}/checkout")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(
        value["detail"],
        "La herramienta ya tiene un préstamo activo o pendiente"
    );

    let (status, value) = get(&app, "/api/v1/loans?status=CHECKOUT_PENDING").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value.as_array().unwrap().len(), 1);
    let (status, value) = get(&app, "/api/v1/operations?status=PENDING").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value.as_array().unwrap().len(), 1);
    assert_eq!(value[0]["loan_id"].as_str().unwrap(), loan_id);

    let actor: Option<Uuid> = query_scalar(
        "SELECT actor_id FROM audit_events WHERE action = 'CHECKOUT_REQUESTED' LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .unwrap()
    .flatten();
    assert_eq!(
        actor,
        Some(lab.admin),
        "operation audit must attribute the actor"
    );
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn concurrent_checkouts_create_a_single_loan() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    let mut authorizations = Vec::new();
    for _ in 0..2 {
        let auth = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
        action(&app, &format!("/api/v1/authorizations/{auth}/approve")).await;
        authorizations.push(auth);
    }

    let mut tasks = Vec::new();
    for auth in authorizations {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            action(&app, &format!("/api/v1/authorizations/{auth}/checkout")).await
        }));
    }
    let mut accepted = 0;
    let mut conflict = 0;
    for task in tasks {
        let (status, value) = task.await.unwrap();
        match status {
            StatusCode::ACCEPTED => accepted += 1,
            StatusCode::CONFLICT => conflict += 1,
            _ => panic!("unexpected status {status}: {value}"),
        }
    }
    assert_eq!(accepted, 1, "two loans started for the same tool");
    assert_eq!(conflict, 1);
    let loans: i64 = count(&pool, "SELECT COUNT(*) FROM loans").await;
    assert_eq!(loans, 1, "concurrent checkouts duplicated loans");
    let operations: i64 = count(&pool, "SELECT COUNT(*) FROM locker_operations").await;
    assert_eq!(operations, 1);
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn return_requires_active_loan_and_free_compartment() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    // Simulate a post-checkout state: active loan, origin placement already removed.
    let loan = Uuid::new_v4();
    query("INSERT INTO loans (id, tool_id, user_id, authorization_id, status, checked_out_at) VALUES ($1, $2, $3, NULL, 'ACTIVE', now())")
        .bind(loan)
        .bind(lab.tool)
        .bind(lab.admin)
        .execute(&pool)
        .await
        .unwrap();
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;
    query("UPDATE tool_placements SET removed_at = now() WHERE tool_id = $1")
        .bind(lab.tool)
        .execute(&pool)
        .await
        .unwrap();

    let missing = Uuid::new_v4();
    let (status, value) = action(&app, &format!("/api/v1/loans/{missing}/return")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");

    let (status, operation) = action(&app, &format!("/api/v1/loans/{loan}/return")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{operation}");
    assert_eq!(operation["command_type"], "RETURN");
    assert_eq!(operation["status"], "PENDING");
    let operation_id = operation["id"].as_str().unwrap().to_owned();
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
        .bind(loan)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loan_status, "RETURN_PENDING");

    // Idempotent retry while the operation is pending.
    let (status, retry) = action(&app, &format!("/api/v1/loans/{loan}/return")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{retry}");
    assert_eq!(
        retry["id"].as_str().unwrap(),
        operation_id,
        "retry duplicated the operation"
    );
    let operations: i64 = count(&pool, "SELECT COUNT(*) FROM locker_operations").await;
    assert_eq!(operations, 1);

    // A second active loan whose destination compartment is occupied must fail (#8).
    let loan2 = Uuid::new_v4();
    query("INSERT INTO loans (id, tool_id, user_id, authorization_id, status, checked_out_at) VALUES ($1, $2, $3, NULL, 'ACTIVE', now())")
        .bind(loan2)
        .bind(lab.tool2)
        .bind(lab.admin)
        .execute(&pool)
        .await
        .unwrap();
    place(&pool, lab.tool2, lab.branch, lab.locker, lab.compartment).await;
    let (status, value) = action(&app, &format!("/api/v1/loans/{loan2}/return")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(value["detail"], "El compartimiento de destino está ocupado");

    // Reserved compartment: a pending RETURN already targets it.
    query(
        "UPDATE tool_placements SET removed_at = now() WHERE tool_id = $1 AND removed_at IS NULL",
    )
    .bind(lab.tool2)
    .execute(&pool)
    .await
    .unwrap();
    query("UPDATE loans SET status = 'ACTIVE' WHERE id = $1")
        .bind(loan2)
        .execute(&pool)
        .await
        .unwrap();
    let (status, value) = action(&app, &format!("/api/v1/loans/{loan2}/return")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(
        value["detail"],
        "El compartimiento está reservado por una devolución pendiente"
    );
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL"]
async fn dashboard_counts_reflect_database() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;
    query("UPDATE tools SET status = 'ON_LOAN' WHERE id = $1")
        .bind(lab.tool2)
        .execute(&pool)
        .await
        .unwrap();
    query("INSERT INTO loans (id, tool_id, user_id, status) VALUES ($1, $2, $3, 'RETURN_PENDING')")
        .bind(Uuid::new_v4())
        .bind(lab.tool2)
        .bind(lab.admin)
        .execute(&pool)
        .await
        .unwrap();

    let (status, value) = get(&app, "/api/v1/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["lockers"], 3);
    assert_eq!(value["tools_available"], 1);
    assert_eq!(value["tools_on_loan"], 1);
    assert_eq!(value["pending_loans"], 1);
    assert_eq!(value["recent_activity"].as_array().unwrap().len(), 0);
}
