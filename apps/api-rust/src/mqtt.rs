//! MQTT bridge (Fase 3): publishes claimed PENDING operations and confirms
//! `COMPARTMENT_OPENED` events. A broker PUBACK means "the broker accepted
//! the command", never "the door opened" (audit #5); only a correlated event
//! confirms physical execution.
//!
//! Ownership (audit #12): operations are partitioned by `origin`; the bridge
//! claims only `origin='rust'` rows, and confirms only its own claimed states
//! (`PUBLISHING`, `PUBLISHED`, `UNKNOWN`). Python-created rows keep `PENDING`
//! and are confirmed by the Python bridge alone, so no pair of components can
//! process the same operation even against a shared database.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use chrono::{SecondsFormat, Utc};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, SubAck, SubscribeReasonCode};
use serde::Deserialize;
use serde_json::json;
use sqlx::{postgres::PgRow, query, query_scalar, PgPool, Row};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::audit::{admin_actor, audit};
use crate::config::MqttSettings;
use crate::presence::{self, PresenceStore};

const EVENTS_FILTER: &str = "locker/+/events";
const STATUS_FILTER: &str = "locker/+/status";
const POLL: Duration = Duration::from_millis(250);
/// A claimed operation whose PUBACK never arrives is indeterminable and
/// becomes UNKNOWN; shortcut: fixed 10s window, no environment knob needed.
const PUBACK_TIMEOUT: Duration = Duration::from_secs(10);
/// TTL sweep cadence: an expired PUBLISHED becomes UNKNOWN within
/// TTL + SWEEP_INTERVAL; no need to hit the database on every 250ms tick.
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);

struct InFlight {
    operation_id: Uuid,
    deadline: Instant,
}

#[derive(Deserialize)]
struct CompartmentEvent {
    #[serde(rename = "type")]
    kind: String,
    correlation_id: String,
    locker_id: String,
    compartment_id: String,
}

/// True when every return code in the SUBACK is a granted QoS: MQTT 3.1.1
/// signals a rejected subscription with 0x80 inside an otherwise normal
/// SUBACK, so the packet alone does not prove the subscription exists.
fn codes_granted(suback: &SubAck) -> bool {
    suback
        .return_codes
        .iter()
        .all(|code| matches!(code, SubscribeReasonCode::Success(_)))
}

/// The events subscription is a single filter: readiness requires exactly
/// one granted code. Empty or multi-code SUBACKs cannot be tied to that one
/// filter and must never satisfy readiness.
fn events_granted(suback: &SubAck) -> bool {
    suback.return_codes.len() == 1 && codes_granted(suback)
}

/// Starts the bridge background task. `subscribed` reflects the events
/// subscription and gates `/health/ready`, mirroring the Python bridge.
/// `presence` collects `locker/+/status` samples for the presence API.
pub fn start(
    pool: PgPool,
    settings: MqttSettings,
    subscribed: Arc<AtomicBool>,
    presence: PresenceStore,
) -> JoinHandle<()> {
    tokio::spawn(run_bridge(pool, settings, subscribed, presence))
}

async fn run_bridge(
    pool: PgPool,
    settings: MqttSettings,
    subscribed: Arc<AtomicBool>,
    presence: PresenceStore,
) {
    // Startup recovery: rows left in PUBLISHING by a previous process never
    // recorded their PUBACK — delivery is indeterminable, so they become
    // UNKNOWN and are never republished (no repeated physical opens). The
    // schema may not exist yet (fresh lab database): retry in the background
    // instead of blocking or dying, so the subscription starts regardless.
    let recovery_pool = pool.clone();
    tokio::spawn(async move {
        while let Err(error) = recover_orphans(&recovery_pool).await {
            tracing::warn!(?error, "mqtt_startup_recovery_pending");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let client_id = format!("{}-{}", settings.client_id, Uuid::new_v4().simple());
    let options = MqttOptions::new(client_id, settings.host.clone(), settings.port);
    loop {
        subscribed.store(false, Ordering::Relaxed);
        let (client, mut eventloop) = AsyncClient::new(options.clone(), 16);
        if let Err(error) = client.subscribe(EVENTS_FILTER, QoS::AtLeastOnce).await {
            // Without the request queued there is no SUBACK to wait for;
            // retry on a fresh connection instead of polling forever with
            // readiness stuck on 503.
            tracing::error!(?error, "mqtt_subscribe_request_failed");
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let mut in_flight: Option<InFlight> = None;
        let mut next_sweep = Instant::now() + SWEEP_INTERVAL;
        loop {
            let polled = tokio::time::timeout(POLL, eventloop.poll()).await;
            match polled {
                Ok(Ok(Event::Incoming(Packet::SubAck(suback)))) => {
                    if !subscribed.load(Ordering::Relaxed) {
                        // First SUBACK on this connection is the events
                        // subscription (status is only requested afterwards),
                        // so only a granted code can satisfy readiness; a
                        // broker rejection (0x80) reconnects and retries.
                        if events_granted(&suback) {
                            subscribed.store(true, Ordering::Relaxed);
                            tracing::info!("mqtt_events_subscribed");
                            if let Err(error) =
                                client.subscribe(STATUS_FILTER, QoS::AtLeastOnce).await
                            {
                                tracing::warn!(?error, "mqtt_status_subscribe_request_failed");
                            }
                        } else {
                            tracing::warn!("mqtt_events_subscription_rejected");
                            // Nothing was published before readiness (the
                            // claim loop gates on `subscribed`), so there is
                            // no in-flight operation to reconcile here.
                            break;
                        }
                    } else if !codes_granted(&suback) {
                        // Status never gates readiness; a rejection only
                        // degrades presence until the next reconnect.
                        tracing::warn!("mqtt_status_subscription_rejected");
                    }
                }
                Ok(Ok(Event::Incoming(Packet::PubAck(_)))) => {
                    // Late PUBACK after an out-of-order event: conditional
                    // UPDATE never overwrites CONFIRMED/FAILED (corrección B).
                    if let Some(in_flight) = in_flight.take() {
                        if let Err(error) = mark_published(&pool, in_flight.operation_id).await {
                            tracing::error!(?error, "mqtt_mark_published_failed");
                        }
                    }
                }
                Ok(Ok(Event::Incoming(Packet::Publish(published)))) => {
                    if let Err(error) = handle_publish(
                        &pool,
                        &presence,
                        published.topic.clone(),
                        published.payload.to_vec(),
                    )
                    .await
                    {
                        tracing::error!(?error, "mqtt_handle_event_failed");
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    // Connection lost; an in-flight publish may or may not
                    // have reached the broker -> indeterminable.
                    if let Some(in_flight) = in_flight.take() {
                        let _ = mark_unknown(
                            &pool,
                            in_flight.operation_id,
                            "PUBACK no confirmado (desconexión)",
                        )
                        .await;
                    }
                    tracing::warn!(?error, "mqtt_disconnected_retrying");
                    break;
                }
                Err(_) => {} // poll tick: periodic work below
            }
            periodic(
                &pool,
                &client,
                &mut in_flight,
                subscribed.load(Ordering::Relaxed),
            )
            .await;
            if Instant::now() >= next_sweep {
                if let Err(error) = expire_published(&pool, settings.confirm_timeout).await {
                    tracing::error!(?error, "mqtt_ttl_sweep_failed");
                }
                next_sweep = Instant::now() + SWEEP_INTERVAL;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Claims the next PENDING operation and publishes it.
/// One operation is in flight at a time, so every PubAck observed on this
/// client belongs to it (single publisher per client). The TTL sweep runs on
/// its own SWEEP_INTERVAL cadence in the bridge loop.
async fn periodic(
    pool: &PgPool,
    client: &AsyncClient,
    in_flight: &mut Option<InFlight>,
    subscribed: bool,
) {
    if let Some(flight) = in_flight.take() {
        if flight.deadline < Instant::now() {
            if let Err(error) =
                mark_unknown(pool, flight.operation_id, "PUBACK no confirmado").await
            {
                tracing::error!(?error, "mqtt_mark_unknown_failed");
            }
            return;
        }
        // Still waiting for the PUBACK: hold the claim, one operation at a time.
        *in_flight = Some(flight);
        return;
    }
    if !subscribed {
        return; // never publish before the events subscription is active
    }
    match claim_next(pool).await {
        Ok(Some(operation)) => match publish_claimed(pool, client, operation, in_flight).await {
            Ok(()) => {}
            Err(error) => tracing::error!(?error, "mqtt_publish_failed"),
        },
        Ok(None) => {}
        Err(error) => tracing::error!(?error, "mqtt_claim_failed"),
    }
}

/// Atomic claim: a single conditional UPDATE (with SKIP LOCKED) moves the row
/// out of PENDING, so concurrent claimants can never double-publish it.
async fn claim_next(pool: &PgPool) -> Result<Option<PgRow>, sqlx::Error> {
    query(
        "UPDATE locker_operations SET status = 'PUBLISHING'
         WHERE id = (SELECT id FROM locker_operations
                     WHERE status = 'PENDING' AND origin = 'rust'
                     ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED)
         RETURNING *",
    )
    .fetch_optional(pool)
    .await
}

async fn publish_claimed(
    pool: &PgPool,
    client: &AsyncClient,
    operation: PgRow,
    in_flight: &mut Option<InFlight>,
) -> Result<(), sqlx::Error> {
    let operation_id: Uuid = operation.try_get("id")?;
    let correlation_id: String = operation.try_get("correlation_id")?;
    let command_type: String = operation.try_get("command_type")?;
    let locker_id: Uuid = operation.try_get("locker_id")?;
    let compartment_id: Uuid = operation.try_get("compartment_id")?;
    let code: String = query_scalar("SELECT code FROM lockers WHERE id = $1")
        .bind(locker_id)
        .fetch_one(pool)
        .await?;
    // Audit #9: the topic addresses lockers by code, which is only unique per
    // branch; an ambiguous code could reach the wrong device, so refuse.
    let duplicates: i64 = query_scalar("SELECT count(*) FROM lockers WHERE code = $1")
        .bind(&code)
        .fetch_one(pool)
        .await?;
    if duplicates > 1 {
        fail_operation(pool, &operation).await?;
        return Ok(());
    }
    let _ = &command_type; // OPEN_COMPARTMENT is the only command in protocol v1.
    let envelope = json!({
        "message_id": Uuid::new_v4().to_string(),
        "correlation_id": correlation_id,
        "protocol_version": "1.0",
        "locker_id": code,
        "compartment_id": compartment_id.to_string(),
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
        "type": "OPEN_COMPARTMENT",
        "payload": {"source": "locker-api-rust"},
    })
    .to_string();
    if let Err(error) = client
        .publish(
            format!("locker/{code}/commands"),
            QoS::AtLeastOnce,
            false,
            envelope,
        )
        .await
    {
        // The command never left the process; safe to release the claim so
        // the next tick retries. QoS 1 redelivery is deduplicated downstream.
        tracing::warn!(?error, operation_id = %operation_id, "mqtt_publish_unavailable");
        let _ = query("UPDATE locker_operations SET status = 'PENDING' WHERE id = $1 AND status = 'PUBLISHING'")
            .bind(operation_id)
            .execute(pool)
            .await;
        return Ok(());
    }
    *in_flight = Some(InFlight {
        operation_id,
        deadline: Instant::now() + PUBACK_TIMEOUT,
    });
    Ok(())
}

/// Records the broker PUBACK. Conditional on `PUBLISHING`: an out-of-order
/// event that already CONFIRMED the operation is never overwritten.
pub async fn mark_published(pool: &PgPool, operation_id: Uuid) -> Result<(), sqlx::Error> {
    query(
        "UPDATE locker_operations SET status = 'PUBLISHED', published_at = now()
         WHERE id = $1 AND status = 'PUBLISHING'",
    )
    .bind(operation_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_unknown(pool: &PgPool, operation_id: Uuid, reason: &str) -> Result<(), sqlx::Error> {
    query(
        "UPDATE locker_operations SET status = 'UNKNOWN', failure_reason = $2
         WHERE id = $1 AND status IN ('PUBLISHING','PUBLISHED')",
    )
    .bind(operation_id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

async fn fail_operation(pool: &PgPool, operation: &PgRow) -> Result<(), sqlx::Error> {
    // The command never reached the broker: the physical outcome is KNOWN
    // (not executed), so the pending loan and its reservation roll back in
    // the same transaction that marks the operation FAILED. Event-time
    // failures are different: the compartment may have opened, so those
    // keep their resources and go through `resolve` instead.
    let operation_id: Uuid = operation.try_get("id")?;
    let mut tx = pool.begin().await?;
    query(
        "UPDATE locker_operations SET status = 'FAILED', failure_reason = $2
           WHERE id = $1 AND status = 'PUBLISHING'",
    )
    .bind(operation_id)
    .bind("Código de locker ambiguo entre sucursales")
    .execute(&mut *tx)
    .await?;
    crate::operations::revert_pending_loan(&mut tx, operation).await?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        "MQTT_FAILED",
        "LockerOperation",
        &operation_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Exposed for integration tests: PUBLISHING rows orphaned by a crash become
/// UNKNOWN on the next startup, without touching loans or placements.
pub async fn recover_orphans(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = query(
        "UPDATE locker_operations SET status = 'UNKNOWN',
                failure_reason = 'PUBACK no confirmado (reinicio)'
         WHERE status = 'PUBLISHING' AND origin = 'rust'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Exposed for integration tests: PUBLISHED operations past the confirmation
/// TTL become UNKNOWN; domain state (loan, tool, placement) is untouched —
/// an uncertain physical outcome must keep its resources blocked.
pub async fn expire_published(pool: &PgPool, ttl: Duration) -> Result<u64, sqlx::Error> {
    let result = query(
        "UPDATE locker_operations SET status = 'UNKNOWN',
                failure_reason = 'Confirmación no recibida en el TTL'
         WHERE status = 'PUBLISHED' AND origin = 'rust'
           AND published_at < now() - ($1 * interval '1 second')",
    )
    .bind(ttl.as_secs() as i64)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Dispatches one incoming publish: status samples feed the presence map
/// (synchronous, no database); event payloads run the confirmation flow.
async fn handle_publish(
    pool: &PgPool,
    presence: &PresenceStore,
    topic: String,
    payload: Vec<u8>,
) -> Result<(), sqlx::Error> {
    if topic.ends_with("/status") {
        presence::record(presence, &topic, &payload);
        return Ok(());
    }
    let event: CompartmentEvent = match serde_json::from_slice(&payload) {
        Ok(event) => event,
        Err(_) => {
            tracing::warn!("mqtt_invalid_event");
            return Ok(());
        }
    };
    if event.kind != "COMPARTMENT_OPENED" {
        return Ok(());
    }
    let Ok(compartment_id) = Uuid::parse_str(&event.compartment_id) else {
        tracing::warn!("mqtt_event_invalid_compartment");
        return Ok(());
    };
    confirm_event(
        pool,
        &event.correlation_id,
        &event.locker_id,
        compartment_id,
    )
    .await
}

/// Confirms a `COMPARTMENT_OPENED` event against the operation it correlates.
/// Duplicates, foreign correlations and events after terminal resolution are
/// ignored; only this bridge's claimed states transition (corrección B/C).
pub async fn confirm_event(
    pool: &PgPool,
    correlation_id: &str,
    locker_code: &str,
    compartment_id: Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let operation: Option<PgRow> =
        query("SELECT * FROM locker_operations WHERE correlation_id = $1 FOR UPDATE")
            .bind(correlation_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(operation) = operation else {
        return Ok(()); // foreign correlation (e.g. Python-owned): never ours
    };
    let status: String = operation.try_get("status")?;
    if !matches!(status.as_str(), "PUBLISHING" | "PUBLISHED" | "UNKNOWN") {
        tracing::warn!(correlation_id, status, "mqtt_event_ignored_not_confirmable");
        return Ok(());
    }
    let operation_id: Uuid = operation.try_get("id")?;
    let command_compartment: Uuid = operation.try_get("compartment_id")?;
    let command_locker: Uuid = operation.try_get("locker_id")?;
    let actual_code: String = query_scalar("SELECT code FROM lockers WHERE id = $1")
        .bind(command_locker)
        .fetch_one(&mut *tx)
        .await?;
    if actual_code != locker_code || command_compartment != compartment_id {
        crate::operations::apply_event_mismatch(&mut tx, &operation).await?;
    } else {
        apply_confirmation(&mut tx, &operation).await?;
    }
    let final_status: String = query_scalar("SELECT status FROM locker_operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&mut *tx)
        .await?;
    let actor = admin_actor(&mut tx).await;
    audit(
        &mut tx,
        &format!("MQTT_{final_status}"),
        "LockerOperation",
        &operation_id.to_string(),
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Applies the domain effects of a confirmed physical execution (mirror of
/// Python's `confirm`): CHECKOUT removes the placement and activates the
/// loan; RETURN places the tool back and closes the loan. Precondition
/// violations fail the operation WITHOUT rolling the loan back: the event
/// proves the compartment opened, so the physical outcome is uncertain and
/// resources are never released automatically — the operator releases them
/// through `POST /operations/{id}/resolve`, which accepts such a FAILED
/// operation while its loan is still pending (audit #17).
pub async fn apply_confirmation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation: &PgRow,
) -> Result<(), sqlx::Error> {
    let operation_id: Uuid = operation.try_get("id")?;
    let command_type: String = operation.try_get("command_type")?;
    let loan_id: Uuid = operation.try_get("loan_id")?;
    let tool_id: Uuid = operation.try_get("tool_id")?;
    let loan: Option<PgRow> = query("SELECT * FROM loans WHERE id = $1")
        .bind(loan_id)
        .fetch_optional(&mut **tx)
        .await?;
    let tool: Option<PgRow> = query("SELECT * FROM tools WHERE id = $1")
        .bind(tool_id)
        .fetch_optional(&mut **tx)
        .await?;
    if loan.is_none() || tool.is_none() {
        crate::operations::apply_event_mismatch(tx, operation).await?;
        return Ok(());
    }
    let loan_status: String = loan.as_ref().unwrap().try_get("status")?;
    match (command_type.as_str(), loan_status.as_str()) {
        ("CHECKOUT", "CHECKOUT_PENDING") => {
            let placement: Option<PgRow> =
                query("SELECT id FROM tool_placements WHERE tool_id = $1 AND removed_at IS NULL")
                    .bind(tool_id)
                    .fetch_optional(&mut **tx)
                    .await?;
            match placement {
                Some(placement) => {
                    let placement_id: Uuid = placement.try_get("id")?;
                    query("UPDATE tool_placements SET removed_at = now() WHERE id = $1")
                        .bind(placement_id)
                        .execute(&mut **tx)
                        .await?;
                    query("UPDATE tools SET status = 'ON_LOAN' WHERE id = $1")
                        .bind(tool_id)
                        .execute(&mut **tx)
                        .await?;
                    query(
                        "UPDATE loans SET status = 'ACTIVE', checked_out_at = now() WHERE id = $1",
                    )
                    .bind(loan_id)
                    .execute(&mut **tx)
                    .await?;
                    query(
                        "UPDATE locker_operations SET status = 'CONFIRMED', confirmed_at = now(),
                                published_at = coalesce(published_at, now())
                         WHERE id = $1 AND status IN ('PUBLISHING','PUBLISHED','UNKNOWN')",
                    )
                    .bind(operation_id)
                    .execute(&mut **tx)
                    .await?;
                }
                None => {
                    mark_failed(tx, operation_id, "Active placement not found").await?;
                }
            }
        }
        ("RETURN", "RETURN_PENDING") => {
            let compartment_id: Uuid = operation.try_get("compartment_id")?;
            // Serialize against `return_loan` and `create_placement`, which
            // take the same row lock before their occupancy checks.
            query("SELECT id FROM compartments WHERE id = $1 FOR UPDATE")
                .bind(compartment_id)
                .fetch_optional(&mut **tx)
                .await?;
            let occupied: Option<Uuid> = query_scalar(
                "SELECT id FROM tool_placements WHERE compartment_id = $1 AND removed_at IS NULL",
            )
            .bind(compartment_id)
            .fetch_optional(&mut **tx)
            .await?;
            if occupied.is_some() {
                mark_failed(tx, operation_id, "Compartment is occupied").await?;
            } else {
                let branch_id: Uuid = operation.try_get("branch_id")?;
                let command_locker: Uuid = operation.try_get("locker_id")?;
                query("INSERT INTO tool_placements (id, tool_id, branch_id, locker_id, compartment_id, reason)
                       VALUES ($1, $2, $3, $4, $5, 'LOAN_RETURN')")
                    .bind(Uuid::new_v4())
                    .bind(tool_id)
                    .bind(branch_id)
                    .bind(command_locker)
                    .bind(compartment_id)
                    .execute(&mut **tx)
                    .await?;
                query("UPDATE tools SET status = 'AVAILABLE' WHERE id = $1")
                    .bind(tool_id)
                    .execute(&mut **tx)
                    .await?;
                query("UPDATE loans SET status = 'RETURNED', returned_at = now() WHERE id = $1")
                    .bind(loan_id)
                    .execute(&mut **tx)
                    .await?;
                query(
                    "UPDATE locker_operations SET status = 'CONFIRMED', confirmed_at = now(),
                            published_at = coalesce(published_at, now())
                     WHERE id = $1 AND status IN ('PUBLISHING','PUBLISHED','UNKNOWN')",
                )
                .bind(operation_id)
                .execute(&mut **tx)
                .await?;
            }
        }
        _ => {
            mark_failed(tx, operation_id, "Unexpected loan state").await?;
        }
    }
    Ok(())
}

async fn mark_failed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    query(
        "UPDATE locker_operations SET status = 'FAILED', confirmed_at = now(), failure_reason = $2
           WHERE id = $1 AND status IN ('PUBLISHING','PUBLISHED','UNKNOWN')",
    )
    .bind(operation_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_suback_with_one_granted_code_satisfies_readiness() {
        let suback = SubAck {
            pkid: 1,
            return_codes: vec![SubscribeReasonCode::Success(QoS::AtLeastOnce)],
        };
        assert!(events_granted(&suback));
    }

    #[test]
    fn rejected_events_suback_never_satisfies_readiness() {
        // MQTT 3.1.1 rejection: 0x80 arrives in an otherwise normal SUBACK,
        // which the old code treated as subscribed -> false readiness.
        let suback = SubAck {
            pkid: 1,
            return_codes: vec![SubscribeReasonCode::Failure],
        };
        assert!(!events_granted(&suback));
    }

    #[test]
    fn malformed_events_suback_never_satisfies_readiness() {
        // Empty or multi-code SUBACKs cannot be tied to the single events
        // filter; treating them as granted would fake readiness.
        let empty = SubAck {
            pkid: 1,
            return_codes: vec![],
        };
        let doubled = SubAck {
            pkid: 1,
            return_codes: vec![
                SubscribeReasonCode::Success(QoS::AtLeastOnce),
                SubscribeReasonCode::Failure,
            ],
        };
        assert!(!events_granted(&empty));
        assert!(!events_granted(&doubled));
    }
}
