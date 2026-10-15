use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use regex::Regex;
use std::sync::LazyLock;

use steno_core::db::Database;
use steno_core::model::Session;
use steno_core::normalize::normalize_command;

use crate::ingest;

static RE_ZSH_EXTENDED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:\s*(\d+):\d+;(.+)$").unwrap());

struct HistoryEntry {
    command: String,
    timestamp: Option<DateTime<Utc>>,
}

pub fn import_if_empty(db: &Database) {
    match db.get_events_count() {
        Ok(count) if count > 0 => {
            tracing::debug!("DB has {} events, skipping history import", count);
            return;
        }
        Err(e) => {
            tracing::warn!("Failed to check event count: {}", e);
            return;
        }
        _ => {}
    }

    tracing::info!("Empty DB detected, importing shell history");

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            tracing::warn!("Could not determine home directory, skipping import");
            return;
        }
    };

    type ShellInfo<'a> = (&'a str, PathBuf, fn(&str) -> Vec<HistoryEntry>);
    let shells: Vec<ShellInfo<'_>> = vec![
        ("bash", home.join(".bash_history"), parse_bash),
        ("zsh", home.join(".zsh_history"), parse_zsh),
        (
            "fish",
            home.join(".local/share/fish/fish_history"),
            parse_fish,
        ),
    ];

    let hostname = gethostname();
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let cwd = home.to_string_lossy().to_string();

    if let Err(e) = db.conn.execute_batch("BEGIN TRANSACTION") {
        tracing::warn!("Failed to begin import transaction: {}", e);
        return;
    }

    let mut total_imported = 0u64;

    for (shell, path, parser) in shells {
        if !path.exists() {
            tracing::debug!("No history file at {}", path.display());
            continue;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", path.display(), e);
                continue;
            }
        };

        let entries = parser(&contents);
        if entries.is_empty() {
            continue;
        }

        let session_id = format!("import-{}-{}", shell, uuid::Uuid::new_v4());

        let first_ts = entries
            .first()
            .and_then(|e| e.timestamp)
            .unwrap_or_else(Utc::now);
        let last_ts = entries
            .last()
            .and_then(|e| e.timestamp)
            .unwrap_or_else(Utc::now);

        let session = Session {
            id: session_id.clone(),
            display_id: 0, // assigned by insert_session
            shell: shell.to_string(),
            pid: 0,
            ppid: None,
            tty: None,
            hostname: hostname.clone(),
            username: username.clone(),
            cwd_start: cwd.clone(),
            started_at: first_ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            ended_at: Some(last_ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            exit_code: Some(0),
            tmux_pane: None,
            ssh_conn: None,
            metadata: Some(r#"{"source":"history-import"}"#.to_string()),
        };

        if let Err(e) = db.insert_session(&session) {
            tracing::warn!("Failed to insert import session for {}: {}", shell, e);
            continue;
        }

        let base_time = first_ts;
        let mut seq = 0u64;
        let mut imported = 0u64;

        for (i, entry) in entries.iter().enumerate() {
            let cmd = entry.command.trim();
            if cmd.is_empty() {
                continue;
            }

            // Apply redaction/ignore rules
            if let Ok(true) = ingest::should_ignore(db, cmd) {
                continue;
            }
            let redacted_cmd = match ingest::apply_redaction(db, cmd) {
                Ok(c) => c,
                Err(_) => cmd.to_string(),
            };
            let was_redacted = redacted_cmd != cmd;
            let normalized = normalize_command(&redacted_cmd);

            let ts = entry
                .timestamp
                .unwrap_or_else(|| base_time + chrono::Duration::seconds(i as i64));
            let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            if let Err(e) = db.insert_event(
                &session_id,
                seq,
                &redacted_cmd,
                Some(&normalized),
                &cwd,
                None,
                None,
                None,
                &ts_str,
                was_redacted,
                false,
            ) {
                tracing::warn!("Failed to insert imported event: {}", e);
            } else {
                imported += 1;
            }

            seq += 1;
        }

        tracing::info!("Imported {} commands from {} history", imported, shell);
        total_imported += imported;
    }

    if let Err(e) = db.conn.execute_batch("COMMIT") {
        tracing::error!("Failed to commit import transaction: {}", e);
        let _ = db.conn.execute_batch("ROLLBACK");
    } else {
        tracing::info!("History import complete: {} total commands", total_imported);
    }
}

fn parse_bash(contents: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut pending_ts: Option<DateTime<Utc>> = None;

    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Ok(epoch) = rest.trim().parse::<i64>() {
                pending_ts = Utc.timestamp_opt(epoch, 0).single();
            }
            continue;
        }

        let cmd = line.trim();
        if !cmd.is_empty() {
            entries.push(HistoryEntry {
                command: cmd.to_string(),
                timestamp: pending_ts.take(),
            });
        }
    }

    entries
}

fn parse_zsh(contents: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();

    for line in contents.lines() {
        if let Some(caps) = RE_ZSH_EXTENDED.captures(line) {
            let epoch: i64 = caps[1].parse().unwrap_or(0);
            let ts = Utc.timestamp_opt(epoch, 0).single();
            let cmd = caps[2].to_string();
            if !cmd.trim().is_empty() {
                entries.push(HistoryEntry {
                    command: cmd,
                    timestamp: ts,
                });
            }
        } else {
            let cmd = line.trim();
            if !cmd.is_empty() {
                entries.push(HistoryEntry {
                    command: cmd.to_string(),
                    timestamp: None,
                });
            }
        }
    }

    entries
}

fn parse_fish(contents: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut current_cmd: Option<String> = None;
    let mut current_ts: Option<DateTime<Utc>> = None;

    for line in contents.lines() {
        if let Some(cmd) = line.strip_prefix("- cmd: ") {
            // Flush previous entry
            if let Some(cmd_text) = current_cmd.take()
                && !cmd_text.trim().is_empty()
            {
                entries.push(HistoryEntry {
                    command: cmd_text,
                    timestamp: current_ts.take(),
                });
            }
            current_cmd = Some(cmd.to_string());
            current_ts = None;
        } else if let Some(when) = line.strip_prefix("  when: ")
            && let Ok(epoch) = when.trim().parse::<i64>()
        {
            current_ts = Utc.timestamp_opt(epoch, 0).single();
        }
    }

    // Flush last entry
    if let Some(cmd_text) = current_cmd
        && !cmd_text.trim().is_empty()
    {
        entries.push(HistoryEntry {
            command: cmd_text,
            timestamp: current_ts,
        });
    }

    entries
}

fn gethostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
