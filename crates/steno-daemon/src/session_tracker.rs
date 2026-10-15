use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use steno_core::db::Database;

pub async fn run(db: Arc<Mutex<Database>>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);

    loop {
        tokio::time::sleep(interval).await;

        let db = db.lock().await;
        match db.get_active_session_pids() {
            Ok(sessions) => {
                for (session_id, pid) in sessions {
                    if !is_process_alive(pid) {
                        let now =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        if let Err(e) = db.end_session(&session_id, &now, None) {
                            tracing::warn!("Failed to end orphaned session {}: {}", session_id, e);
                        } else {
                            tracing::info!(
                                "Ended orphaned session {} (pid {} no longer alive)",
                                session_id,
                                pid
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check active sessions: {}", e);
            }
        }
    }
}

fn is_process_alive(pid: u32) -> bool {
    // Use kill(pid, 0) to check if process exists
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}
