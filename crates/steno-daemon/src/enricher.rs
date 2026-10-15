use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use steno_core::config::StenoConfig;
use steno_core::db::Database;

struct GitCache {
    entries: HashMap<PathBuf, CacheEntry>,
}

struct CacheEntry {
    branch: String,
    root: PathBuf,
    last_checked: Instant,
    last_hit: Instant,
}

pub async fn run(db: Arc<Mutex<Database>>, config: &StenoConfig) {
    let cache_ttl = Duration::from_secs(config.git_cache_ttl_secs);
    let evict_after = Duration::from_secs(300); // 5 minutes without hits
    let mut cache = GitCache {
        entries: HashMap::new(),
    };

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Get events needing enrichment
        let events = {
            let db = db.lock().await;
            match db.get_events_needing_enrichment(50) {
                Ok(events) => events,
                Err(e) => {
                    tracing::warn!("Failed to get events for enrichment: {}", e);
                    continue;
                }
            }
        };

        if events.is_empty() {
            continue;
        }

        // Evict stale cache entries
        let now = Instant::now();
        cache
            .entries
            .retain(|_, entry| now.duration_since(entry.last_hit) < evict_after);

        for (event_id, cwd) in &events {
            let cwd_path = PathBuf::from(cwd);

            // Check cache first
            let cached = find_in_cache(&mut cache, &cwd_path, cache_ttl);

            let (branch, root) = if let Some((b, r)) = cached {
                (Some(b), Some(r))
            } else {
                // Lookup git info
                match lookup_git_info(&cwd_path).await {
                    Some((branch, root)) => {
                        // Cache the result
                        cache.entries.insert(
                            root.clone(),
                            CacheEntry {
                                branch: branch.clone(),
                                root: root.clone(),
                                last_checked: Instant::now(),
                                last_hit: Instant::now(),
                            },
                        );
                        (Some(branch), Some(root.to_string_lossy().to_string()))
                    }
                    None => (None, None),
                }
            };

            // Update the event
            let db = db.lock().await;
            if let Err(e) = db.enrich_event(*event_id, branch.as_deref(), root.as_deref()) {
                tracing::warn!("Failed to enrich event {}: {}", event_id, e);
            }
        }
    }
}

fn find_in_cache(cache: &mut GitCache, cwd: &Path, ttl: Duration) -> Option<(String, String)> {
    for (root, entry) in &mut cache.entries {
        if cwd.starts_with(root) {
            let now = Instant::now();
            if now.duration_since(entry.last_checked) < ttl {
                entry.last_hit = now;
                return Some((
                    entry.branch.clone(),
                    entry.root.to_string_lossy().to_string(),
                ));
            }
        }
    }
    None
}

async fn lookup_git_info(cwd: &Path) -> Option<(String, PathBuf)> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

    let branch_output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .await
        .ok()?;

    if !branch_output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    Some((branch, root))
}
