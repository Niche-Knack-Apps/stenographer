mod enricher;
mod gc;
mod grouper;
mod handler;
mod importer;
mod ingest;
mod server;
mod session_tracker;
mod spool_reader;

use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};
use tracing_subscriber::EnvFilter;

use steno_core::config;
use steno_core::db::Database;
use steno_core::model::SpoolEvent;

#[tokio::main]
async fn main() {
    let config = config::load_config();

    // Initialize logging
    let log_dir = config::log_dir(&config);
    std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");

    let file_appender = tracing_appender::rolling::daily(&log_dir, "stenographerd.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(non_blocking)
        .init();

    tracing::info!("stenographerd v{} starting", env!("CARGO_PKG_VERSION"));

    // Create data directories
    let db_path = config::db_path(&config);
    let runtime_dir = config::runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("Failed to create runtime directory");

    // Set runtime dir permissions to 0700
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700));
    }

    // Open database
    let db = Database::open(&db_path).expect("Failed to open database");

    // Import shell history on first run (empty DB)
    importer::import_if_empty(&db);

    let db = Arc::new(Mutex::new(db));

    // Write PID file
    let pid_path = config::pid_path();
    std::fs::write(&pid_path, std::process::id().to_string()).expect("Failed to write PID file");

    // Create spool file if it doesn't exist
    let spool_path = config::spool_path();
    if !spool_path.exists() {
        std::fs::write(&spool_path, "").expect("Failed to create spool file");
    }

    // Event channel from spool reader to ingest
    let (event_tx, event_rx) = mpsc::channel::<SpoolEvent>(1024);

    // Spawn spool reader
    let spool_db = Arc::clone(&db);
    let spool_path_clone = spool_path.clone();
    let spool_handle = tokio::spawn(async move {
        spool_reader::run(spool_path_clone, spool_db, event_tx).await;
    });

    // Spawn ingest pipeline
    let ingest_db = Arc::clone(&db);
    let ingest_config = config.clone();
    let ingest_handle = tokio::spawn(async move {
        ingest::run(event_rx, ingest_db, &ingest_config).await;
    });

    // Spawn enricher
    let enricher_db = Arc::clone(&db);
    let enricher_config = config.clone();
    let enricher_handle = tokio::spawn(async move {
        enricher::run(enricher_db, &enricher_config).await;
    });

    // Spawn RPC server
    let server_db = Arc::clone(&db);
    let server_config = config.clone();
    let socket_path = config::socket_path();
    let server_handle = tokio::spawn(async move {
        server::run(socket_path, server_db, server_config).await;
    });

    // Spawn session monitor
    let monitor_db = Arc::clone(&db);
    let monitor_interval = config.session_check_interval_secs;
    let monitor_handle = tokio::spawn(async move {
        session_tracker::run(monitor_db, monitor_interval).await;
    });

    // Spawn grouper
    let grouper_db = Arc::clone(&db);
    let grouper_interval = config.grouper_interval_secs;
    let grouper_handle = tokio::spawn(async move {
        grouper::run(grouper_db, grouper_interval).await;
    });

    // Spawn GC
    let gc_db = Arc::clone(&db);
    let gc_config = config.clone();
    let gc_handle = tokio::spawn(async move {
        gc::run(gc_db, &gc_config).await;
    });

    tracing::info!("All services started");

    // Wait for shutdown signal
    let shutdown = tokio::signal::ctrl_c();
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("Failed to register SIGTERM handler");

    tokio::select! {
        _ = shutdown => {
            tracing::info!("Received SIGINT, shutting down");
        }
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM, shutting down");
        }
    }

    // Graceful shutdown
    spool_handle.abort();
    ingest_handle.abort();
    enricher_handle.abort();
    server_handle.abort();
    monitor_handle.abort();
    grouper_handle.abort();
    gc_handle.abort();

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    let socket_path = config::socket_path();
    let _ = std::fs::remove_file(&socket_path);

    tracing::info!("stenographerd stopped");
}
