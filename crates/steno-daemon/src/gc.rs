use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use steno_core::config::StenoConfig;
use steno_core::db::Database;

pub async fn run(db: Arc<Mutex<Database>>, config: &StenoConfig) {
    let interval = Duration::from_secs(config.gc_interval_secs);
    let retention_days = config.retention_days;

    // Wait before first GC run
    tokio::time::sleep(Duration::from_secs(60)).await;

    loop {
        {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
            let cutoff_str = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            let db = db.lock().await;
            match db.gc_events(&cutoff_str, false) {
                Ok((events, sessions, freed)) => {
                    if events > 0 || sessions > 0 {
                        tracing::info!(
                            "GC: deleted {} events, {} sessions, freed {} bytes",
                            events,
                            sessions,
                            freed
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("GC failed: {}", e);
                }
            }

            // Compact spool file
            let spool_path = steno_core::config::spool_path();
            if let Ok(metadata) = std::fs::metadata(&spool_path) {
                // If spool is larger than 10MB and all data is ingested, truncate
                if metadata.len() > 10_000_000
                    && let Ok(offset) = db.get_spool_offset()
                    && offset >= metadata.len()
                {
                    let _ = std::fs::write(&spool_path, "");
                    let _ = db.set_spool_offset(0);
                    tracing::info!("Compacted spool file");
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}
