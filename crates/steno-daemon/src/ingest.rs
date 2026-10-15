use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};

use steno_core::config::StenoConfig;
use steno_core::db::Database;
use steno_core::model::{Session, SpoolEvent};
use steno_core::normalize::normalize_command;

pub async fn run(
    mut event_rx: mpsc::Receiver<SpoolEvent>,
    db: Arc<Mutex<Database>>,
    config: &StenoConfig,
) {
    let flush_interval = Duration::from_millis(config.batch_flush_ms);
    let batch_max = config.batch_max_events;
    let mut batch: Vec<SpoolEvent> = Vec::with_capacity(batch_max);

    loop {
        let deadline = tokio::time::sleep(flush_interval);
        tokio::pin!(deadline);

        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(e) => {
                        batch.push(e);
                        if batch.len() >= batch_max {
                            flush_batch(&db, &mut batch).await;
                        }
                    }
                    None => {
                        // Channel closed, flush remaining and exit
                        flush_batch(&db, &mut batch).await;
                        break;
                    }
                }
            }
            _ = &mut deadline => {
                if !batch.is_empty() {
                    flush_batch(&db, &mut batch).await;
                }
            }
        }
    }
}

async fn flush_batch(db: &Arc<Mutex<Database>>, batch: &mut Vec<SpoolEvent>) {
    if batch.is_empty() {
        return;
    }

    let db = db.lock().await;
    let events = std::mem::take(batch);

    for event in events {
        if let Err(e) = process_event(&db, event) {
            tracing::warn!("Failed to process event: {}", e);
        }
    }
}

fn process_event(db: &Database, event: SpoolEvent) -> steno_core::Result<()> {
    match event {
        SpoolEvent::Start {
            sid,
            pid,
            shell,
            tty,
            cwd,
            host,
            user,
            ts,
            ppid,
            tmux_pane,
            ssh_conn,
        } => {
            let session = Session {
                id: sid,
                display_id: 0, // assigned by insert_session
                shell,
                pid,
                ppid,
                tty,
                hostname: host,
                username: user,
                cwd_start: cwd,
                started_at: ts,
                ended_at: None,
                exit_code: None,
                tmux_pane,
                ssh_conn,
                metadata: None,
            };
            db.insert_session(&session)?;
            tracing::debug!("Session started: {}", session.id);
        }
        SpoolEvent::Pre {
            sid,
            seq,
            cmd,
            cwd,
            ts,
            ccwd,
        } => {
            // Apply redaction rules
            let redacted_cmd = apply_redaction(db, &cmd)?;
            let was_redacted = redacted_cmd != cmd;

            // Check ignore rules
            if should_ignore(db, &cmd)? {
                tracing::debug!("Ignoring command: {}", cmd);
                return Ok(());
            }

            // Rewrite resolvable relative paths to absolute, rooted at the
            // recorded cwd, so history never stores ambiguous relative paths.
            let abs_cmd = steno_core::normalize::absolutize_command_paths(&redacted_cmd, &cwd);

            // Normalize for mining
            let normalized = normalize_command(&abs_cmd);

            // Canonical cwd from shell hook
            let canonical_cwd = ccwd.as_deref();

            // Parse cd target if command starts with "cd "
            let (cd_target_raw, cd_target_canonical) = parse_cd_target(&cmd, &cwd);

            db.insert_event(
                &sid,
                seq,
                &abs_cmd,
                Some(&normalized),
                &cwd,
                canonical_cwd,
                cd_target_raw.as_deref(),
                cd_target_canonical.as_deref(),
                &ts,
                was_redacted,
                true, // incomplete until post arrives
            )?;
            tracing::debug!("Event pre: sid={} seq={}", sid, seq);
        }
        SpoolEvent::Post { sid, seq, exit, ts } => {
            db.complete_event(&sid, seq, exit, &ts)?;
            tracing::debug!("Event post: sid={} seq={} exit={}", sid, seq, exit);
        }
        SpoolEvent::End { sid, exit, ts } => {
            db.end_session(&sid, &ts, exit)?;
            tracing::debug!("Session ended: {}", sid);
        }
    }

    Ok(())
}

pub(crate) fn apply_redaction(db: &Database, cmd: &str) -> steno_core::Result<String> {
    let mut stmt = db
        .conn
        .prepare_cached("SELECT pattern, replacement FROM redaction_rules WHERE enabled = 1")?;

    let rules: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = cmd.to_string();
    for (pattern, replacement) in &rules {
        if let Ok(re) = regex::Regex::new(pattern) {
            result = re.replace_all(&result, replacement.as_str()).to_string();
        }
    }

    Ok(result)
}

pub(crate) fn should_ignore(db: &Database, cmd: &str) -> steno_core::Result<bool> {
    let mut stmt = db
        .conn
        .prepare_cached("SELECT pattern, match_type FROM ignore_rules WHERE enabled = 1")?;

    let rules: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (pattern, match_type) in &rules {
        match match_type.as_str() {
            "glob" => {
                // Simple glob: * matches anything
                let regex_pattern = format!(
                    "^{}$",
                    regex::escape(pattern)
                        .replace(r"\*", ".*")
                        .replace(r"\?", ".")
                );
                if let Ok(re) = regex::Regex::new(&regex_pattern)
                    && re.is_match(cmd)
                {
                    return Ok(true);
                }
            }
            "regex" => {
                if let Ok(re) = regex::Regex::new(pattern)
                    && re.is_match(cmd)
                {
                    return Ok(true);
                }
            }
            "prefix" => {
                if cmd.starts_with(pattern.as_str()) {
                    return Ok(true);
                }
            }
            _ => {}
        }
    }

    Ok(false)
}

fn parse_cd_target(cmd: &str, cwd: &str) -> (Option<String>, Option<String>) {
    let trimmed = cmd.trim();
    if !trimmed.starts_with("cd ") && trimmed != "cd" {
        return (None, None);
    }

    let target = trimmed.strip_prefix("cd ").unwrap_or("").trim();
    if target.is_empty() || target == "-" || target == "~" {
        return (Some(target.to_string()), None);
    }

    // Strip surrounding quotes
    let target = target
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| target.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(target);

    let raw = target.to_string();
    let path = std::path::Path::new(target);
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::Path::new(cwd).join(path)
    };

    let canonical = std::fs::canonicalize(&abs_path)
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    (Some(raw), canonical)
}
