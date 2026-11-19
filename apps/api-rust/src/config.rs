use std::{env, net::SocketAddr, time::Duration};

use sqlx::{postgres::PgPoolOptions, PgPool};

#[derive(Clone)]
pub struct Settings {
    pub database_url: String,
    pub bind: SocketAddr,
    pub max_connections: u32,
    pub timeout: Duration,
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
        if max_connections == 0 {
            return Err("pool size must be positive");
        }
        Ok(Self {
            database_url,
            bind,
            max_connections,
            timeout,
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
