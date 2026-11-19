use sqlx::{query_scalar, PgConnection};
use uuid::Uuid;

/// Audit trail with actor attribution (audit #10). The lab panel is a single
/// admin session, so mutations are attributed to the admin user until Phase 5
/// introduces real sessions.
pub async fn admin_actor(conn: &mut PgConnection) -> Option<Uuid> {
    query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_optional(conn)
        .await
        .ok()
        .flatten()
}

pub async fn audit(
    conn: &mut PgConnection,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, action, entity_type, entity_id, actor_id, metadata_json)
         VALUES ($1, $2, $3, $4, $5, $6::json)",
    )
    .bind(Uuid::new_v4())
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(actor)
    .bind(serde_json::json!({}).to_string())
    .execute(conn)
    .await?;
    Ok(())
}
