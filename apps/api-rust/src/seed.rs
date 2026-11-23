//! Schema baseline migration and explicit `LAB` seed (F4.3/F4.4).
//!
//! Replaces Alembic and `app.modules.seed` from the retired Python backend
//! with two subcommands on the single `locker-api-rust` binary:
//! - `locker-api-rust migrate`: applies `migrations/20261123000001_baseline.sql`
//!   idempotently (safe on existing databases and fresh ones alike).
//! - `locker-api-rust seed`: requires `APP_ENV=lab` and a completely empty
//!   domain database; populates the `LAB` branch, `LAB-LOCKER-001`, 24
//!   compartments/locks/tools/placements, camera, and 3 lab users (`admin:admin`).

use sqlx::{query, query_scalar, raw_sql, PgPool};
use uuid::Uuid;

pub const BASELINE_SQL: &str = include_str!("../migrations/20261123000001_baseline.sql");

const DOMAIN_TABLES: &[&str] = &[
    "branches",
    "roles",
    "users",
    "memberships",
    "lockers",
    "compartments",
    "locker_locks",
    "cameras",
    "tools",
    "tool_placements",
    "authorizations",
    "loans",
    "locker_operations",
    "audit_events",
];

/// Applies the SQLx baseline schema inside one transaction.
pub async fn migrate(pool: &PgPool) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    raw_sql(BASELINE_SQL)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Checks `APP_ENV=lab` and seeds the `LAB` dataset into an empty database.
pub async fn seed_from_env(pool: &PgPool) -> Result<(), String> {
    if std::env::var("APP_ENV").ok().as_deref() != Some("lab") {
        return Err("La inicialización solo se permite con APP_ENV=lab".to_owned());
    }
    seed_lab(pool).await
}

/// Seeds the `LAB` dataset; refuses any database that already contains domain rows.
pub async fn seed_lab(pool: &PgPool) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let lock_sql = format!(
        "LOCK TABLE {} IN ACCESS EXCLUSIVE MODE",
        DOMAIN_TABLES.join(", ")
    );
    raw_sql(&lock_sql)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for table in DOMAIN_TABLES {
        let has_row: bool = query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} LIMIT 1)"))
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        if has_row {
            return Err("La base de laboratorio debe estar vacía".to_owned());
        }
    }

    let admin_role = Uuid::new_v4();
    let coordinator_role = Uuid::new_v4();
    let mechanic_role = Uuid::new_v4();
    for (id, code, name) in [
        (admin_role, "ADMIN", "Admin"),
        (coordinator_role, "COORDINATOR", "Coordinator"),
        (mechanic_role, "MECHANIC", "Mechanic"),
    ] {
        query("INSERT INTO roles (id, code, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(code)
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    let branch_id = Uuid::new_v4();
    query(
        "INSERT INTO branches (id, code, name, status) VALUES ($1, 'LAB', 'Laboratorio', 'ACTIVE')",
    )
    .bind(branch_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let locker_id = Uuid::new_v4();
    query(
        "INSERT INTO lockers (id, branch_id, code, name, status)
         VALUES ($1, $2, 'LAB-LOCKER-001', 'Locker de laboratorio', 'ONLINE')",
    )
    .bind(locker_id)
    .bind(branch_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    for position in 1..=24_i32 {
        let code = format!("C{position:02}");
        let compartment_id = Uuid::new_v4();
        query(
            "INSERT INTO compartments (id, locker_id, code, name, position, status)
             VALUES ($1, $2, $3, $4, $5, 'ACTIVE')",
        )
        .bind(compartment_id)
        .bind(locker_id)
        .bind(&code)
        .bind(&code)
        .bind(position)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        query(
            "INSERT INTO locker_locks (id, compartment_id, hardware_address, status)
             VALUES ($1, $2, $3, 'LOCKED')",
        )
        .bind(Uuid::new_v4())
        .bind(compartment_id)
        .bind(format!("SIM-{code}"))
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        let asset_code = format!("TOOL-{position:03}");
        let tool_id = Uuid::new_v4();
        query(
            "INSERT INTO tools (id, asset_code, name, rfid_tag, status)
             VALUES ($1, $2, $3, $4, 'AVAILABLE')",
        )
        .bind(tool_id)
        .bind(&asset_code)
        .bind(format!("Herramienta {position:03}"))
        .bind(format!("RFID-{asset_code}"))
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        query(
            "INSERT INTO tool_placements (id, tool_id, branch_id, locker_id, compartment_id, reason)
             VALUES ($1, $2, $3, $4, $5, 'INITIAL_ASSIGNMENT')",
        )
        .bind(Uuid::new_v4())
        .bind(tool_id)
        .bind(branch_id)
        .bind(locker_id)
        .bind(compartment_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    query("INSERT INTO cameras (id, locker_id, name, status) VALUES ($1, $2, 'Cámara simulada', 'SIMULATED')")
        .bind(Uuid::new_v4())
        .bind(locker_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (username, display_name, role_id) in [
        ("admin", "Administrador", admin_role),
        ("coordinator", "Coordinador", coordinator_role),
        ("mechanic", "Mecánico", mechanic_role),
    ] {
        let user_id = Uuid::new_v4();
        query(
            "INSERT INTO users (id, username, display_name, password, status)
             VALUES ($1, $2, $3, 'admin', 'ACTIVE')",
        )
        .bind(user_id)
        .bind(username)
        .bind(display_name)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        query(
            "INSERT INTO memberships (id, user_id, branch_id, role_id, status)
             VALUES ($1, $2, $3, $4, 'ACTIVE')",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(branch_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}
