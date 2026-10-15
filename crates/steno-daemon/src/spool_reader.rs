use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{Mutex, mpsc};

use steno_core::db::Database;
use steno_core::model::SpoolEvent;

pub async fn run(
    spool_path: PathBuf,
    db: Arc<Mutex<Database>>,
    event_tx: mpsc::Sender<SpoolEvent>,
) {
    // Get initial offset from database
    let offset = {
        let db = db.lock().await;
        db.get_spool_offset().unwrap_or(0)
    };

    // Read any existing data from offset
    if let Ok(new_offset) = read_from_offset(&spool_path, offset, &event_tx).await {
        let db = db.lock().await;
        let _ = db.set_spool_offset(new_offset);
    }

    // Set up inotify watcher
    let (notify_tx, mut notify_rx) = mpsc::channel(64);

    let _watcher = {
        let notify_tx = notify_tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res
                    && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                {
                    let _ = notify_tx.blocking_send(());
                }
            },
            notify::Config::default(),
        )
        .expect("Failed to create file watcher");

        // Watch the parent directory (in case spool file is recreated)
        if let Some(parent) = spool_path.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .expect("Failed to watch spool directory");
        }

        watcher
    };

    let mut current_offset = {
        let db = db.lock().await;
        db.get_spool_offset().unwrap_or(0)
    };

    loop {
        // Wait for file change notification
        if notify_rx.recv().await.is_none() {
            break;
        }

        // Drain any queued notifications
        while notify_rx.try_recv().is_ok() {}

        match read_from_offset(&spool_path, current_offset, &event_tx).await {
            Ok(new_offset) => {
                if new_offset != current_offset {
                    current_offset = new_offset;
                    let db = db.lock().await;
                    let _ = db.set_spool_offset(current_offset);
                }
            }
            Err(e) => {
                tracing::warn!("Error reading spool: {}", e);
            }
        }
    }
}

async fn read_from_offset(
    path: &PathBuf,
    offset: u64,
    tx: &mpsc::Sender<SpoolEvent>,
) -> std::io::Result<u64> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    // If file is smaller than offset, it was truncated — reset
    let actual_offset = if metadata.len() < offset { 0 } else { offset };

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(actual_offset))?;

    let mut current_pos = actual_offset;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        current_pos += bytes_read as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<SpoolEvent>(trimmed) {
            Ok(event) => {
                if tx.send(event).await.is_err() {
                    tracing::warn!("Event channel closed");
                    break;
                }
            }
            Err(e) => {
                tracing::warn!("Failed to parse spool line: {} — {}", e, trimmed);
            }
        }
    }

    Ok(current_pos)
}
