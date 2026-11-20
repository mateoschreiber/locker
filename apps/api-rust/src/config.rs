use std::{env, net::SocketAddr, time::Duration};

use sqlx::{postgres::PgPoolOptions, PgPool};

#[derive(Clone)]
pub struct Settings {
    pub database_url: String,
    pub bind: SocketAddr,
    pub max_connections: u32,
    pub timeout: Duration,
    pub mqtt: MqttSettings,
}

/// MQTT bridge configuration; mirrors the Python bridge contract (host, port,
/// QoS 1) with lab-safe defaults. `enabled=false` keeps the API serving HTTP
/// with operations left PENDING (parity harness uses this).
#[derive(Clone)]
pub struct MqttSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub confirm_timeout: Duration,
    /// Presence window (F4.1): status messages older than this at read time
    /// report STALE instead of ONLINE; LOCKER_OFFLINE always reports OFFLINE.
    pub stale_seconds: u64,
}

impl MqttSettings {
    fn from_env() -> Result<Self, String> {
        let enabled = env::var("RUST_MQTT_ENABLED")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
            .unwrap_or(true);
        let host = env::var("RUST_MQTT_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        let port = env::var("RUST_MQTT_PORT")
            .unwrap_or_else(|_| "1883".to_owned())
            .parse::<u16>()
            .map_err(|_| "invalid RUST_MQTT_PORT".to_owned())?;
        let client_id =
            env::var("RUST_MQTT_CLIENT_ID").unwrap_or_else(|_| "locker-api-rust".to_owned());
        let seconds = env::var("RUST_MQTT_CONFIRM_TIMEOUT_SECONDS")
            .unwrap_or_else(|_| "60".to_owned())
            .parse::<u64>()
            .map_err(|_| "invalid RUST_MQTT_CONFIRM_TIMEOUT_SECONDS".to_owned())?;
        if !(1..=600).contains(&seconds) {
            return Err("RUST_MQTT_CONFIRM_TIMEOUT_SECONDS must be between 1 and 600".to_owned());
        }
        let stale_seconds = env::var("RUST_PRESENCE_STALE_SECONDS")
            .unwrap_or_else(|_| crate::presence::DEFAULT_STALE_SECONDS.to_string())
            .parse::<u64>()
            .map_err(|_| "invalid RUST_PRESENCE_STALE_SECONDS".to_owned())?;
        if !(30..=600).contains(&stale_seconds) {
            return Err("RUST_PRESENCE_STALE_SECONDS must be between 30 and 600".to_owned());
        }
        Ok(Self {
            enabled,
            host,
            port,
            client_id,
            confirm_timeout: Duration::from_secs(seconds),
            stale_seconds,
        })
    }
}

impl Settings {
    pub fn from_env() -> Result<Self, &'static str> {
        let database_url =
            env::var("RUST_DATABASE_URL").map_err(|_| "RUST_DATABASE_URL required")?;
        let bind = env::var("RUST_BIND_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1:8001".to_owned())
            .parse()
            .map_err(|_| "invalid RUST_BIND_ADDRESS")?;
        let max_connections = env::var("RUST_DB_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "5".to_owned())
            .parse::<u32>()
            .map_err(|_| "invalid RUST_DB_MAX_CONNECTIONS")?;
        let timeout =
            parse_timeout(&env::var("RUST_DB_TIMEOUT_SECONDS").unwrap_or_else(|_| "3".to_owned()))?;
        let mqtt = MqttSettings::from_env().map_err(|_| {
            "invalid MQTT configuration (RUST_MQTT_HOST/PORT/CONFIRM_TIMEOUT_SECONDS)"
        })?;
        if max_connections == 0 {
            return Err("pool size must be positive");
        }
        Ok(Self {
            database_url,
            bind,
            max_connections,
            timeout,
            mqtt,
        })
    }

    pub fn pool(&self) -> Result<PgPool, sqlx::Error> {
        PgPoolOptions::new()
            .max_connections(self.max_connections)
            .acquire_timeout(self.timeout)
            .connect_lazy(&self.database_url)
    }
}

fn parse_timeout(value: &str) -> Result<Duration, &'static str> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "invalid RUST_DB_TIMEOUT_SECONDS")?;
    if !(1..=30).contains(&seconds) {
        return Err("RUST_DB_TIMEOUT_SECONDS must be between 1 and 30");
    }
    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::{parse_timeout, Settings};
    use std::time::Duration;

    #[tokio::test]
    async fn invalid_database_url_is_rejected() {
        let settings = Settings {
            database_url: "not-a-postgres-url".to_owned(),
            bind: "127.0.0.1:0".parse().unwrap(),
            max_connections: 2,
            timeout: Duration::from_secs(1),
            mqtt: super::MqttSettings {
                enabled: false,
                host: "127.0.0.1".to_owned(),
                port: 1883,
                client_id: "test".to_owned(),
                confirm_timeout: Duration::from_secs(60),
                stale_seconds: 90,
            },
        };
        assert!(settings.pool().is_err());
    }

    #[test]
    fn timeout_is_bounded() {
        assert_eq!(parse_timeout("1"), Ok(Duration::from_secs(1)));
        assert_eq!(parse_timeout("30"), Ok(Duration::from_secs(30)));
        for value in ["0", "31", "invalid"] {
            assert!(parse_timeout(value).is_err(), "{value}");
        }
    }
}
