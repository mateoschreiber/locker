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
    (
        pool.clone(),
        router(pool, Duration::from_secs(5), None),
        guard,
    )
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
async fn concurrent_returns_to_one_compartment_create_a_single_operation() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    // Two active loans whose tools both came out of the same compartment.
    let tool3 = Uuid::new_v4();
    query("INSERT INTO tools (id, asset_code, name, status) VALUES ($1, 'TOOL-003', 'T', 'AVAILABLE')")
        .bind(tool3)
        .execute(&pool)
        .await
        .unwrap();
    let mut loans = Vec::new();
    for tool in [lab.tool, tool3] {
        let loan = Uuid::new_v4();
        query("INSERT INTO loans (id, tool_id, user_id, authorization_id, status, checked_out_at) VALUES ($1, $2, $3, NULL, 'ACTIVE', now())")
            .bind(loan)
            .bind(tool)
            .bind(lab.admin)
            .execute(&pool)
            .await
            .unwrap();
        place(&pool, tool, lab.branch, lab.locker, lab.compartment).await;
        query("UPDATE tool_placements SET removed_at = now() WHERE tool_id = $1")
            .bind(tool)
            .execute(&pool)
            .await
            .unwrap();
        loans.push(loan);
    }

    let mut tasks = Vec::new();
    for loan in loans {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            action(&app, &format!("/api/v1/loans/{loan}/return")).await
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
    assert_eq!(
        accepted, 1,
        "two pending returns accepted for one compartment"
    );
    assert_eq!(conflict, 1);
    let returns: i64 = count(
        &pool,
        "SELECT COUNT(*) FROM locker_operations WHERE command_type = 'RETURN'",
    )
    .await;
    assert_eq!(
        returns, 1,
        "compartment reservation race duplicated operations"
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

// ---------------------------------------------------------------------------
// Fase 3: MQTT publication, confirmation, recovery and reconciliation.
// These tests exercise a REAL broker (LOCKER_TEST_MQTT_HOST, default
// 127.0.0.1:1883); the bridge runs in-process and a rumqttc client acts as
// the simulator, so no mock hides integration failures.
// ---------------------------------------------------------------------------

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use locker_api_rust::{mqtt, MqttSettings};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
struct SimCommand {
    locker_code: String,
    compartment_id: String,
    correlation_id: String,
    raw: Value,
}

fn mqtt_settings() -> MqttSettings {
    MqttSettings {
        enabled: true,
        host: std::env::var("LOCKER_TEST_MQTT_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned()),
        port: std::env::var("LOCKER_TEST_MQTT_PORT")
            .unwrap_or_else(|_| "1883".to_owned())
            .parse()
            .expect("valid LOCKER_TEST_MQTT_PORT"),
        client_id: format!("test-bridge-{}", Uuid::new_v4().simple()),
        confirm_timeout: Duration::from_secs(60),
    }
}

/// A rumqttc client standing in for the locker simulator: it subscribes to
/// every `commands` topic and reports each command on a channel so tests
/// decide when (and whether) to answer.
async fn start_simulator(settings: &MqttSettings) -> (AsyncClient, mpsc::Receiver<SimCommand>) {
    let options = MqttOptions::new(
        format!("test-simulator-{}", Uuid::new_v4().simple()),
        settings.host.clone(),
        settings.port,
    );
    let (client, mut eventloop) = AsyncClient::new(options, 16);
    client
        .subscribe("locker/+/commands", QoS::AtLeastOnce)
        .await
        .expect("simulator subscribe");
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(published))) => {
                    let Ok(raw) = serde_json::from_slice::<Value>(&published.payload) else {
                        continue;
                    };
                    let command = SimCommand {
                        locker_code: raw["locker_id"].as_str().unwrap_or_default().to_owned(),
                        compartment_id: raw["compartment_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        correlation_id: raw["correlation_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        raw,
                    };
                    if tx.send(command).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    (client, rx)
}

/// Mirror of the simulator's COMPARTMENT_OPENED envelope.
async fn respond_opened(client: &AsyncClient, command: &SimCommand) {
    let envelope = json!({
        "message_id": Uuid::new_v4().to_string(),
        "correlation_id": command.correlation_id,
        "protocol_version": "1.0",
        "locker_id": command.locker_code,
        "compartment_id": command.compartment_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "type": "COMPARTMENT_OPENED",
        "payload": {"simulated": true},
    });
    client
        .publish(
            format!("locker/{}/events", command.locker_code),
            QoS::AtLeastOnce,
            false,
            envelope.to_string(),
        )
        .await
        .expect("publish COMPARTMENT_OPENED");
}

/// Polls the database until an operation reaches the wanted status.
async fn wait_for_status(pool: &PgPool, correlation_id: &str, wanted: &str) {
    for _ in 0..150 {
        let status: Option<String> =
            query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
                .bind(correlation_id)
                .fetch_optional(pool)
                .await
                .unwrap();
        if status.as_deref() == Some(wanted) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("operation {correlation_id} never reached {wanted}");
}

/// Next broker command with a hard timeout so regressions fail fast
/// instead of hanging the suite.
async fn next_command(commands: &mut mpsc::Receiver<SimCommand>) -> SimCommand {
    tokio::time::timeout(Duration::from_secs(15), commands.recv())
        .await
        .expect("no OPEN_COMPARTMENT arrived on the broker")
        .expect("simulator channel closed")
}

/// Fixture helper: force an operation into a state, bypassing guards.
async fn force_status(pool: &PgPool, correlation_id: &str, status: &str) {
    query("UPDATE locker_operations SET status = $2 WHERE correlation_id = $1")
        .bind(correlation_id)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
}

async fn force_published_stale(pool: &PgPool, correlation_id: &str) {
    query(
        "UPDATE locker_operations SET status = 'PUBLISHED',
                published_at = now() - interval '1 hour'
         WHERE correlation_id = $1",
    )
    .bind(correlation_id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn mqtt_checkout_and_return_confirm_via_broker() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    // locker2 has the globally-unique code "L2"; "L1" exists in two branches
    // and the #9 publish-time check would refuse it.
    place(&pool, lab.tool, lab.branch, lab.locker2, lab.compartment2).await;

    let settings = mqtt_settings();
    let (simulator, mut commands) = start_simulator(&settings).await;
    let flag = Arc::new(AtomicBool::new(false));
    let bridge = mqtt::start(pool.clone(), settings.clone(), flag.clone());

    // CHECKOUT: HTTP -> claim -> publish -> event -> confirm.
    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (status, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{operation}");
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();

    let command = next_command(&mut commands).await;
    assert_eq!(command.raw["type"], "OPEN_COMPARTMENT");
    assert_eq!(command.raw["protocol_version"], "1.0");
    assert_eq!(command.raw["locker_id"], "L2");
    assert_eq!(command.raw["compartment_id"], lab.compartment2.to_string());
    assert_eq!(command.raw["payload"]["source"], "locker-api-rust");
    assert_eq!(command.raw["correlation_id"], correlation);
    assert!(command.raw["message_id"].is_string());

    respond_opened(&simulator, &command).await;
    wait_for_status(&pool, &correlation, "CONFIRMED").await;
    assert!(flag.load(Ordering::Relaxed), "bridge never subscribed");
    let tool_status: String = query_scalar("SELECT status FROM tools WHERE id = $1")
        .bind(lab.tool)
        .fetch_one(&pool)
        .await
        .unwrap();
    let loan: (Uuid, String) = sqlx::query_as(
        "SELECT id, status FROM loans WHERE tool_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(lab.tool)
    .fetch_one(&pool)
    .await
    .unwrap();
    let placement: i64 = query_scalar(
        "SELECT COUNT(*) FROM tool_placements WHERE tool_id = $1 AND removed_at IS NULL",
    )
    .bind(lab.tool)
    .fetch_one(&pool)
    .await
    .unwrap();
    let published_at: Option<String> =
        query_scalar("SELECT published_at::text FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert_eq!(tool_status, "ON_LOAN");
    assert_eq!(loan.1, "ACTIVE");
    assert_eq!(placement, 0, "checkout removed the origin placement");
    assert!(published_at.is_some(), "confirmed implies published");

    // RETURN: same protocol path back into the compartment.
    let (status, return_op) = action(&app, &format!("/api/v1/loans/{}/return", loan.0)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{return_op}");
    let command = commands.recv().await.expect("return OPEN_COMPARTMENT");
    respond_opened(&simulator, &command).await;
    wait_for_status(&pool, command.correlation_id.as_str(), "CONFIRMED").await;
    let tool_status: String = query_scalar("SELECT status FROM tools WHERE id = $1")
        .bind(lab.tool)
        .fetch_one(&pool)
        .await
        .unwrap();
    let placement: i64 = query_scalar(
        "SELECT COUNT(*) FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
    )
    .bind(lab.compartment2)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tool_status, "AVAILABLE");
    assert_eq!(placement, 1, "return did not place the tool back");
    bridge.abort();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn duplicate_opened_event_has_no_double_effect() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker2, lab.compartment2).await;

    let settings = mqtt_settings();
    let (simulator, mut commands) = start_simulator(&settings).await;
    let flag = Arc::new(AtomicBool::new(false));
    let bridge = mqtt::start(pool.clone(), settings.clone(), flag);

    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let operation_id = operation["id"].as_str().unwrap().to_owned();
    let command = next_command(&mut commands).await;
    // QoS 1 may deliver the same event twice; only one confirmation applies.
    respond_opened(&simulator, &command).await;
    respond_opened(&simulator, &command).await;
    wait_for_status(&pool, command.correlation_id.as_str(), "CONFIRMED").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let audits: i64 = count(
        &pool,
        &format!("SELECT COUNT(*) FROM audit_events WHERE action = 'MQTT_CONFIRMED' AND entity_id = '{operation_id}'"),
    )
    .await;
    assert_eq!(audits, 1, "duplicate event re-confirmed");
    let tool_status: String = query_scalar("SELECT status FROM tools WHERE id = $1")
        .bind(lab.tool)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tool_status, "ON_LOAN");
    bridge.abort();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn event_before_puback_confirms_and_late_puback_never_overwrites() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    // Simulated in-flight claim: the event races ahead of the PUBACK.
    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let operation_id: Uuid = Uuid::parse_str(operation["id"].as_str().unwrap()).unwrap();
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();
    force_status(&pool, &correlation, "PUBLISHING").await;

    mqtt::confirm_event(&pool, &correlation, "L1", lab.compartment)
        .await
        .unwrap();
    let status: String = query_scalar("SELECT status FROM locker_operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "CONFIRMED", "early event did not confirm");
    let published_at: Option<String> =
        query_scalar("SELECT published_at::text FROM locker_operations WHERE id = $1")
            .bind(operation_id)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(published_at.is_some(), "confirmation implies publication");

    // The late PUBACK must not overwrite CONFIRMED (corrección B).
    mqtt::mark_published(&pool, operation_id).await.unwrap();
    let status: String = query_scalar("SELECT status FROM locker_operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "CONFIRMED");
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn late_event_confirms_unknown_after_ttl() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();
    force_published_stale(&pool, &correlation).await;

    // TTL expiry never touches the domain: the loan keeps blocking the tool.
    let expired = mqtt::expire_published(&pool, Duration::from_secs(1))
        .await
        .unwrap();
    assert!(expired >= 1);
    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "UNKNOWN");
    let loan_status: String =
        query_scalar("SELECT status FROM loans WHERE tool_id = $1 ORDER BY created_at DESC")
            .bind(lab.tool)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(loan_status, "CHECKOUT_PENDING");

    // A late COMPARTMENT_OPENED still proves execution and confirms.
    mqtt::confirm_event(&pool, &correlation, "L1", lab.compartment)
        .await
        .unwrap();
    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "CONFIRMED");
    let loan_status: String =
        query_scalar("SELECT status FROM loans WHERE tool_id = $1 ORDER BY created_at DESC")
            .bind(lab.tool)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(loan_status, "ACTIVE");
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn restart_publishes_pending_and_marks_orphan_publishing_unknown() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    // Globally-unique locker code so the #9 check does not refuse the flows.
    place(&pool, lab.tool, lab.branch, lab.locker2, lab.compartment2).await;
    place(&pool, lab.tool2, lab.branch, lab.locker, lab.compartment).await;

    // Op A: created but never published (crash between commit and claim).
    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let correlation_a = operation["correlation_id"].as_str().unwrap().to_owned();

    // Op B: claimed by a previous process that died before its PUBACK.
    let authorization_b = create_authorization(&app, lab.tool2, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization_b}/approve"),
    )
    .await;
    let (status_b, operation_b) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization_b}/checkout"),
    )
    .await;
    assert_eq!(status_b, StatusCode::ACCEPTED, "{operation_b}");
    let correlation_b = operation_b["correlation_id"].as_str().unwrap().to_owned();
    force_status(&pool, &correlation_b, "PUBLISHING").await;

    let settings = mqtt_settings();
    let (simulator, mut commands) = start_simulator(&settings).await;
    let flag = Arc::new(AtomicBool::new(false));
    let bridge = mqtt::start(pool.clone(), settings.clone(), flag);

    // Restart behavior: A is published once (and completes), B is UNKNOWN and
    // is NEVER republished (no uncertain second physical open).
    let command = next_command(&mut commands).await;
    assert_eq!(command.correlation_id, correlation_a);
    respond_opened(&simulator, &command).await;
    wait_for_status(&pool, &correlation_a, "CONFIRMED").await;
    let status_b: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation_b)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_b, "UNKNOWN", "orphan claim did not become UNKNOWN");
    let failure_reason: String =
        query_scalar("SELECT failure_reason FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation_b)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(failure_reason.contains("PUBACK"));
    // No second command ever arrives for B.
    let rejected = tokio::time::timeout(Duration::from_millis(800), commands.recv()).await;
    assert!(rejected.is_err(), "orphan operation was republished");
    bridge.abort();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn python_origin_operations_are_never_claimed_by_rust() {
    let (pool, _app, _guard) = setup().await;
    let lab = seed(&pool).await;

    // A PENDING operation the Python API created on a shared database.
    let python_loan = Uuid::new_v4();
    query(
        "INSERT INTO loans (id, tool_id, user_id, status) VALUES ($1, $2, $3, 'CHECKOUT_PENDING')",
    )
    .bind(python_loan)
    .bind(lab.tool)
    .bind(lab.admin)
    .execute(&pool)
    .await
    .unwrap();
    query(
        "INSERT INTO locker_operations (id, correlation_id, command_type, status,
            tool_id, loan_id, branch_id, locker_id, compartment_id, origin)
         VALUES ($1, 'python-op-1', 'CHECKOUT', 'PENDING', $2, $3, $4, $5, $6, 'python')",
    )
    .bind(Uuid::new_v4())
    .bind(lab.tool)
    .bind(python_loan)
    .bind(lab.branch)
    .bind(lab.locker)
    .bind(lab.compartment)
    .execute(&pool)
    .await
    .unwrap();

    let settings = mqtt_settings();
    let (_simulator, mut commands) = start_simulator(&settings).await;
    let flag = Arc::new(AtomicBool::new(false));
    let bridge = mqtt::start(pool.clone(), settings.clone(), flag);
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = 'python-op-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "PENDING", "rust claimed a python-owned operation");
    let rejected = tokio::time::timeout(Duration::from_millis(400), commands.recv()).await;
    assert!(
        rejected.is_err(),
        "python-owned operation was published by rust"
    );

    // And the rust bridge ignores foreign correlation ids in events.
    mqtt::confirm_event(&pool, "python-op-1", "L1", lab.compartment)
        .await
        .unwrap();
    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = 'python-op-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "PENDING", "rust confirmed a python-owned operation");
    bridge.abort();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn ambiguous_locker_code_fails_without_publishing() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    // lab.locker and lab.locker3 share code "L1" across branches (audit #9).
    place(&pool, lab.tool2, lab.branch2, lab.locker3, lab.compartment3).await;

    let settings = mqtt_settings();
    let (_simulator, mut commands) = start_simulator(&settings).await;
    let flag = Arc::new(AtomicBool::new(false));
    let bridge = mqtt::start(pool.clone(), settings.clone(), flag);

    let authorization = create_authorization(&app, lab.tool2, lab.admin, lab.branch2).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();
    wait_for_status(&pool, &correlation, "FAILED").await;
    let reason: String =
        query_scalar("SELECT failure_reason FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(reason.contains("ambiguo"), "{reason}");
    let rejected = tokio::time::timeout(Duration::from_millis(400), commands.recv()).await;
    assert!(rejected.is_err(), "ambiguous locker code was published");
    bridge.abort();
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn resolve_endpoint_guards_reverts_and_survives_late_events() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    let authorization = create_authorization(&app, lab.tool, lab.admin, lab.branch).await;
    action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/approve"),
    )
    .await;
    let (_, operation) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    let operation_id = operation["id"].as_str().unwrap().to_owned();
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();
    force_status(&pool, &correlation, "UNKNOWN").await;
    let loan_id: Uuid =
        query_scalar("SELECT loan_id FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();

    let uri = format!("/api/v1/operations/{operation_id}/resolve");

    // Authorization, verdict, reason and evidence are all mandatory.
    let (status, value) = post(
        &app,
        &uri,
        json!({"outcome": "FAILED", "reason": "r", "evidence": "e"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    let (status, value) = request(
        app.clone(),
        "POST",
        &uri,
        Some(json!({"outcome": "FAILED", "reason": "r", "evidence": "e"})),
    )
    .await;
    let _ = value; // without the bearer header (request() adds none)
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let authorized = |body: Value| {
        let app = app.clone();
        let uri = uri.clone();
        async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&uri)
                        .header("authorization", "Bearer lab-admin-token")
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };

    let (status, value) =
        authorized(json!({"outcome": "MAYBE", "reason": "r", "evidence": "e"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    let (status, value) =
        authorized(json!({"outcome": "FAILED", "reason": "  ", "evidence": "e"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    let (status, value) = authorized(json!({"outcome": "FAILED", "reason": "r"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");

    // Only UNKNOWN operations are resolvable.
    force_status(&pool, &correlation, "PENDING").await;
    let (status, value) =
        authorized(json!({"outcome": "FAILED", "reason": "r", "evidence": "e"})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    force_status(&pool, &correlation, "UNKNOWN").await;

    // FAILED verdict: the operator attests no physical execution; the loan
    // rolls back, the authorization becomes reusable and everything is audited.
    let (status, value) = authorized(json!({
        "outcome": "FAILED",
        "reason": "verificado en sitio",
        "evidence": "compartimento cerrado, herramienta presente"
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "FAILED");
    assert_eq!(value["failure_reason"], "verificado en sitio");
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
        .bind(loan_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loan_status, "CANCELLED");
    let auth_status: String = query_scalar("SELECT status FROM authorizations WHERE id = $1")
        .bind(authorization)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        auth_status, "APPROVED",
        "authorization not reusable after resolve"
    );
    let reason: String = query_scalar(
        "SELECT metadata_json->>'reason' FROM audit_events WHERE action = 'OPERATION_RESOLVED_FAILED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason, "verificado en sitio");

    // A late MQTT event after resolution is ignored (documented rule).
    mqtt::confirm_event(&pool, &correlation, "L1", lab.compartment)
        .await
        .unwrap();
    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "FAILED");
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
        .bind(loan_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        loan_status, "CANCELLED",
        "late event reverted the resolution"
    );

    // The reverted authorization can drive a fresh checkout.
    let (status, _value) = action(
        &app,
        &format!("/api/v1/authorizations/{authorization}/checkout"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // `reason` lands in failure_reason VARCHAR(255). With the operation now
    // terminal, the exact limit passes validation (refused only by state as
    // 409) while one character over is a contract 422, never a database 500.
    let exact = "x".repeat(255);
    let (status, _) =
        authorized(json!({"outcome": "FAILED", "reason": exact, "evidence": "e"})).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "255 chars must pass validation"
    );
    let over = "x".repeat(256);
    let (status, value) =
        authorized(json!({"outcome": "FAILED", "reason": over, "evidence": "e"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    assert_eq!(
        value["detail"],
        "Campo 'reason': excede el máximo de 255 caracteres"
    );
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn resolve_confirmed_attests_physical_execution() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    // Post-checkout fixture with the return operation stuck in UNKNOWN.
    let loan = Uuid::new_v4();
    query("INSERT INTO loans (id, tool_id, user_id, status) VALUES ($1, $2, $3, 'ACTIVE')")
        .bind(loan)
        .bind(lab.tool)
        .bind(lab.admin)
        .execute(&pool)
        .await
        .unwrap();
    query("UPDATE tools SET status = 'ON_LOAN' WHERE id = $1")
        .bind(lab.tool)
        .execute(&pool)
        .await
        .unwrap();
    query("UPDATE tool_placements SET removed_at = now() WHERE tool_id = $1")
        .bind(lab.tool)
        .execute(&pool)
        .await
        .unwrap();
    let (status, operation) = action(&app, &format!("/api/v1/loans/{loan}/return")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{operation}");
    let operation_id = operation["id"].as_str().unwrap().to_owned();
    let correlation = operation["correlation_id"].as_str().unwrap().to_owned();
    force_status(&pool, &correlation, "UNKNOWN").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/operations/{operation_id}/resolve"))
                .header("authorization", "Bearer lab-admin-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "outcome": "CONFIRMED",
                        "reason": "verificado en sitio",
                        "evidence": "herramienta devuelta, compartimento ocupado"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: String =
        query_scalar("SELECT status FROM locker_operations WHERE correlation_id = $1")
            .bind(&correlation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "CONFIRMED");
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
        .bind(loan)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loan_status, "RETURNED");
    let tool_status: String = query_scalar("SELECT status FROM tools WHERE id = $1")
        .bind(lab.tool)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tool_status, "AVAILABLE");
    let placement: i64 = query_scalar(
        "SELECT COUNT(*) FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
    )
    .bind(lab.compartment)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(placement, 1);
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn resolve_confirmed_that_cannot_apply_fails_and_releases_the_loan() {
    let (pool, app, _guard) = setup().await;
    let lab = seed(&pool).await;
    place(&pool, lab.tool, lab.branch, lab.locker, lab.compartment).await;

    // Post-checkout fixture with a stuck return operation, and another tool
    // occupying the destination compartment when the operator attests.
    let loan = Uuid::new_v4();
    query("INSERT INTO loans (id, tool_id, user_id, status) VALUES ($1, $2, $3, 'ACTIVE')")
        .bind(loan)
        .bind(lab.tool)
        .bind(lab.admin)
        .execute(&pool)
        .await
        .unwrap();
    query("UPDATE tools SET status = 'ON_LOAN' WHERE id = $1")
        .bind(lab.tool)
        .execute(&pool)
        .await
        .unwrap();
    query("UPDATE tool_placements SET removed_at = now() WHERE tool_id = $1")
        .bind(lab.tool)
        .execute(&pool)
        .await
        .unwrap();
    // The return is requested while the compartment is free...
    let (status, operation) = action(&app, &format!("/api/v1/loans/{loan}/return")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{operation}");
    // ...and something lands there before the resolution (direct SQL, like a
    // state change the API's reservation could not see coming).
    place(&pool, lab.tool2, lab.branch, lab.locker, lab.compartment).await;
    let operation_id = operation["id"].as_str().unwrap().to_owned();
    force_status(
        &pool,
        operation["correlation_id"].as_str().unwrap(),
        "UNKNOWN",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/operations/{operation_id}/resolve"))
                .header("authorization", "Bearer lab-admin-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "outcome": "CONFIRMED",
                        "reason": "verificado en sitio",
                        "evidence": "afirmación contradicha por el inventario"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    // The response exposes the resulting status and reason, so the operator
    // sees that the attestation could not be applied (Fase 3.6, etapa 3.7).
    assert_eq!(value["status"], "FAILED", "{value}");
    assert_eq!(
        value["failure_reason"], "Compartment is occupied",
        "{value}"
    );
    // The pending loan rolls back instead of hanging without an operation.
    let loan_status: String = query_scalar("SELECT status FROM loans WHERE id = $1")
        .bind(loan)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        loan_status, "ACTIVE",
        "loan left RETURN_PENDING with no operation"
    );
    // The audit records the verdict AND the resulting status.
    let audited: String = query_scalar(
        "SELECT metadata_json->>'status' FROM audit_events WHERE action = 'OPERATION_RESOLVED_CONFIRMED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, "FAILED");
}

#[tokio::test]
#[ignore = "requires LOCKER_TEST_DATABASE_URL and isolated PostgreSQL/MQTT"]
async fn readiness_gates_on_mqtt_subscription_flag() {
    let (pool, _app, _guard) = setup().await;
    let flag = Arc::new(AtomicBool::new(false));
    let app = router(pool.clone(), Duration::from_secs(5), Some(flag.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    flag.store(true, Ordering::Relaxed);
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
