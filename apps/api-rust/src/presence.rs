//! Observed locker presence from `locker/{code}/status` messages (F4.1-A).
//!
//! The state is intentionally in-memory: presence is observational, not
//! domain state; a backend restart recovers it from the broker's retained
//! messages, whose timestamps are re-validated so an old retained ONLINE can
//! never present a device as connected. Stale-ness is computed at read time,
//! so heartbeats cost no database writes and no periodic sweeps.
//! shortcut: the map grows per unique code seen on the anonymous broker; a
//! real deployment (F5: per-device identity) must bound or authenticate it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::State, Json};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, query, query_scalar, PgPool, Row};
use uuid::Uuid;

use crate::error::ApiError;

/// Messages older than this (at read time) are STALE instead of ONLINE;
/// configurable through `RUST_PRESENCE_STALE_SECONDS` (30..=600).
pub const DEFAULT_STALE_SECONDS: u64 = 90;
/// A status timestamp further in the future is invalid evidence (clock skew).
const SKEW_SECONDS: i64 = 30;

/// Last valid status message for one locker code.
#[derive(Clone)]
struct PresenceSample {
    kind: String,
    timestamp: DateTime<Utc>,
    source: String,
}

pub struct PresenceInner {
    samples: Mutex<HashMap<String, PresenceSample>>,
    stale_seconds: i64,
}

pub type PresenceStore = Arc<PresenceInner>;

pub fn new_store(stale: Duration) -> PresenceStore {
    Arc::new(PresenceInner {
        samples: Mutex::new(HashMap::new()),
        stale_seconds: stale.as_secs().max(1) as i64,
    })
}

#[derive(Deserialize)]
struct StatusEvent {
    #[serde(rename = "type")]
    kind: String,
    timestamp: String,
}

/// Observed presence derived from one sample. OFFLINE is known and sticky;
/// otherwise the age against the stale window decides, so an old retained
/// ONLINE message reads as STALE, never as connected.
fn derive(kind: &str, age_seconds: i64, stale_seconds: i64) -> &'static str {
    if kind == "LOCKER_OFFLINE" {
        "OFFLINE"
    } else if age_seconds > stale_seconds {
        "STALE"
    } else {
        "ONLINE"
    }
}

/// Records one `locker/{code}/status` payload. Malformed messages, unknown
/// event types and implausible timestamps are ignored; unknown locker codes
/// are stored but never surfaced (the read side only lists real lockers).
pub fn record(store: &PresenceStore, topic: &str, payload: &[u8]) {
    let Some(code) = topic
        .strip_prefix("locker/")
        .and_then(|rest| rest.strip_suffix("/status"))
    else {
        return;
    };
    let Ok(event) = serde_json::from_slice::<StatusEvent>(payload) else {
        tracing::warn!(topic, "mqtt_status_invalid_event");
        return;
    };
    if !matches!(
        event.kind.as_str(),
        "LOCKER_ONLINE" | "LOCKER_HEARTBEAT" | "LOCKER_OFFLINE"
    ) {
        return;
    }
    let Ok(timestamp) = DateTime::parse_from_rfc3339(&event.timestamp) else {
        tracing::warn!(topic, "mqtt_status_invalid_timestamp");
        return;
    };
    let timestamp = timestamp.with_timezone(&Utc);
    if timestamp > Utc::now() + chrono::Duration::seconds(SKEW_SECONDS) {
        tracing::warn!(topic, "mqtt_status_future_timestamp");
        return;
    }
    store
        .samples
        .lock()
        .expect("presence store poisoned")
        .insert(
            code.to_owned(),
            PresenceSample {
                kind: event.kind,
                timestamp,
                source: topic.to_owned(),
            },
        );
}

/// GET /lockers/presence — one row per locker with the observed presence.
/// Never invents availability: without data the state is UNKNOWN with nulls.
pub async fn lockers_presence(
    State((pool, store)): State<(PgPool, PresenceStore)>,
) -> Result<Json<Value>, ApiError> {
    let rows: Vec<PgRow> = query("SELECT id, code, name, status FROM lockers ORDER BY created_at")
        .fetch_all(&pool)
        .await?;
    // Codes shared between branches cannot be attributed (audit #9): those
    // lockers read UNKNOWN so the wrong device is never shown online.
    let ambiguous: Vec<String> =
        query_scalar("SELECT code FROM lockers GROUP BY code HAVING count(*) > 1")
            .fetch_all(&pool)
            .await?;
    let samples = store
        .samples
        .lock()
        .expect("presence store poisoned")
        .clone();
    let now = Utc::now();
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get("id")?;
        let code: String = row.try_get("code")?;
        let name: String = row.try_get("name")?;
        let sample = if ambiguous.contains(&code) {
            None
        } else {
            samples.get(&code)
        };
        let (presence, last, age, source) = match sample {
            None => ("UNKNOWN", None, None, None),
            Some(sample) => {
                let age = (now - sample.timestamp).num_seconds().max(0);
                (
                    derive(sample.kind.as_str(), age, store.stale_seconds),
                    Some(
                        sample
                            .timestamp
                            .to_rfc3339_opts(SecondsFormat::Micros, true),
                    ),
                    Some(age),
                    Some(sample.source.clone()),
                )
            }
        };
        items.push(json!({
            "locker_id": id.to_string(),
            "code": code,
            "name": name,
            "administrative_status": row.try_get::<String, _>("status")?,
            "presence": presence,
            "last_communication": last,
            "age_seconds": age,
            "source": source,
            "ambiguous_code": ambiguous.contains(&code),
            "updated_at": last,
        }));
    }
    Ok(Json(Value::Array(items)))
}

/// GET /capabilities — explicit feature detection: the panel enables presence
/// and reconciliation only when the connected backend supports them. Python
/// does not implement this route (404 → features off); never send resolve or
/// presence requests to a backend that does not advertise them.
pub async fn capabilities() -> Json<Value> {
    Json(json!({ "resolve": true, "presence": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_ignores_malformed_and_invalid_messages() {
        let store = new_store(Duration::from_secs(90));
        record(&store, "locker/L1/status", b"not json");
        record(
            &store,
            "locker/L1/status",
            br#"{"type":"LOCKER_HEARTBEAT","timestamp":"not-a-date"}"#,
        );
        record(
            &store,
            "locker/L1/status",
            br#"{"type":"BANANA","timestamp":"2026-01-01T00:00:00Z"}"#,
        );
        // A timestamp far in the future is invalid evidence.
        record(
            &store,
            "locker/L1/status",
            br#"{"type":"LOCKER_HEARTBEAT","timestamp":"2999-01-01T00:00:00Z"}"#,
        );
        // Only `locker/{code}/status` topics carry presence.
        record(
            &store,
            "locker/L1/events",
            br#"{"type":"LOCKER_HEARTBEAT","timestamp":"2026-01-01T00:00:00Z"}"#,
        );
        assert!(store.samples.lock().unwrap().is_empty());
        record(
            &store,
            "locker/L1/status",
            br#"{"type":"LOCKER_HEARTBEAT","timestamp":"2026-01-01T00:00:00Z"}"#,
        );
        assert_eq!(store.samples.lock().unwrap().len(), 1);
    }

    #[test]
    fn presence_derivation_follows_the_stale_window() {
        // Fresh evidence: ONLINE; aged evidence: STALE; never fakes ONLINE
        // from an old retained message; OFFLINE is known and sticky.
        assert_eq!(derive("LOCKER_HEARTBEAT", 10, 90), "ONLINE");
        assert_eq!(derive("LOCKER_ONLINE", 10, 90), "ONLINE");
        assert_eq!(derive("LOCKER_ONLINE", 3600, 90), "STALE");
        assert_eq!(derive("LOCKER_HEARTBEAT", 91, 90), "STALE");
        assert_eq!(derive("LOCKER_OFFLINE", 3600, 90), "OFFLINE");
        assert_eq!(derive("LOCKER_OFFLINE", 5, 90), "OFFLINE");
    }
}
