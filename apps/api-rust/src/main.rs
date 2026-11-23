use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;

use locker_api_rust::{mqtt, router, seed, Settings};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let settings = Settings::from_env()?;
    let pool = settings
        .pool()
        .map_err(|_| "invalid PostgreSQL configuration")?;
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => {
            seed::migrate(&pool).await?;
            tracing::info!("database schema migration complete");
            pool.close().await;
            return Ok(());
        }
        Some("seed") => {
            seed::seed_from_env(&pool).await?;
            tracing::info!("laboratory LAB seed complete");
            pool.close().await;
            return Ok(());
        }
        Some(other) => {
            return Err(
                format!("unknown subcommand '{other}' (expected 'migrate' or 'seed')").into(),
            );
        }
        None => {}
    }
    let mqtt_ready = Arc::new(AtomicBool::new(false));
    // Presence lives with the bridge; the endpoint reads the same samples.
    let presence = locker_api_rust::new_store(Duration::from_secs(settings.mqtt.stale_seconds));
    if settings.mqtt.enabled {
        mqtt::start(
            pool.clone(),
            settings.mqtt.clone(),
            mqtt_ready.clone(),
            presence.clone(),
        );
    }
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(address = %settings.bind, mqtt = settings.mqtt.enabled, "rust backend started");
    // Readiness gates on the events subscription only while the bridge runs.
    let ready_flag = if settings.mqtt.enabled {
        Some(mqtt_ready)
    } else {
        None
    };
    axum::serve(
        listener,
        router(pool.clone(), settings.timeout, ready_flag, presence),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    pool.close().await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
