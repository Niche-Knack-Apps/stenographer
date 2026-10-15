use serde_json::{Value, json};

use steno_core::config::StenoConfig;
use steno_core::db::Database;
use steno_core::ipc::methods::Method;
use steno_core::ipc::protocol::{
    INTERNAL_ERROR, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
};

pub fn dispatch(request: &JsonRpcRequest, db: &Database, config: &StenoConfig) -> JsonRpcResponse {
    let result = match request.method.as_str() {
        Method::DAEMON_STATUS => handle_status(db, config),
        Method::QUERY_SEARCH => handle_search(db, &request.params),
        Method::QUERY_TAIL => handle_tail(db, &request.params),
        Method::QUERY_CONTEXT => handle_context(db, &request.params),
        Method::SESSION_LIST => handle_session_list(db, &request.params),
        Method::SESSION_SHOW => handle_session_show(db, &request.params),
        Method::SESSION_CURRENT => handle_session_current(db),
        Method::TASK_LIST => handle_task_list(db),
        Method::TASK_SHOW => handle_task_show(db, &request.params),
        Method::TASK_DELETE => handle_task_delete(db, &request.params),
        Method::PRIVACY_MODE_GET => handle_privacy_mode_get(config),
        Method::PRIVACY_MODE_SET => Ok(json!({"ok": true})),
        Method::PRIVACY_RULES_LIST => handle_privacy_rules_list(db),
        Method::PRIVACY_RULES_ADD => handle_privacy_rules_add(db, &request.params),
        Method::PRIVACY_RULES_REMOVE => handle_privacy_rules_remove(db, &request.params),
        Method::PRIVACY_REDACT => handle_privacy_redact(db, &request.params),
        Method::EXPORT_JSON => handle_export_json(db, &request.params),
        Method::IMPORT_EVENTS => handle_import_events(db, &request.params),
        Method::GC_RUN => handle_gc_run(db, &request.params),
        _ => {
            return JsonRpcResponse::error(
                request.id.clone(),
                METHOD_NOT_FOUND,
                format!("Method not found: {}", request.method),
            );
        }
    };

    match result {
        Ok(value) => JsonRpcResponse::success(request.id.clone(), value),
        Err(e) => JsonRpcResponse::error(request.id.clone(), INTERNAL_ERROR, e.to_string()),
    }
}

fn handle_status(db: &Database, _config: &StenoConfig) -> steno_core::Result<Value> {
    let events_total = db.get_events_count()?;
    let sessions = db.list_sessions(true, 1000, 0)?;
    let db_size = db.db_size_bytes()?;
    let spool_offset = db.get_spool_offset()?;

    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "sessions_active": sessions.len(),
        "events_total": events_total,
        "db_size_bytes": db_size,
        "spool_offset": spool_offset,
    }))
}

fn handle_search(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let pattern = params["pattern"].as_str().unwrap_or("*");
    let cwd = params["cwd"].as_str();
    let session = params["session"].as_str();
    let since = params["since"].as_str();
    let until = params["until"].as_str();
    let exit_code = params["exit_code"].as_i64().map(|v| v as i32);
    let limit = params["limit"].as_i64().unwrap_or(50);
    let dedup = params["deduplicate"].as_bool().unwrap_or(false);

    let events = if dedup {
        db.search_events_dedup(pattern, cwd, session, since, until, exit_code, limit)?
    } else {
        db.search_events(pattern, cwd, session, since, until, exit_code, limit)?
    };
    Ok(json!({ "events": events }))
}

fn handle_tail(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let session = params["session"].as_str();
    let cwd = params["cwd"].as_str();
    let limit = params["limit"].as_i64().unwrap_or(20);
    let dedup = params["deduplicate"].as_bool().unwrap_or(false);

    let events = if dedup {
        db.tail_events_dedup(session, cwd, limit)?
    } else {
        db.tail_events(session, limit)?
    };
    Ok(json!({ "events": events }))
}

fn handle_session_list(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let active = params["active"].as_bool().unwrap_or(false);
    let limit = params["limit"].as_u64().unwrap_or(50) as u32;
    let offset = params["offset"].as_u64().unwrap_or(0) as u32;
    let sessions = db.list_sessions(active, limit, offset)?;
    Ok(json!({ "sessions": sessions }))
}

fn handle_session_show(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let id = params["id"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'id' parameter".into()))?;
    let session = db.resolve_session(id)?;
    let events = db.get_session_events(&session.id)?;
    Ok(json!({ "session": session, "events": events }))
}

fn handle_session_current(db: &Database) -> steno_core::Result<Value> {
    let sessions = db.list_sessions(true, 100, 0)?;
    Ok(json!({ "sessions": sessions }))
}

fn handle_task_list(db: &Database) -> steno_core::Result<Value> {
    let tasks = db.list_auto_tasks()?;
    Ok(json!({ "tasks": tasks }))
}

fn handle_task_show(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'name' parameter".into()))?;
    let task = db.get_auto_task_by_name(name)?;
    let events = if let Some(id) = task.db_id {
        db.get_task_events(id)?
    } else {
        vec![]
    };
    Ok(json!({ "task": task, "events": events }))
}

fn handle_task_delete(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'name' parameter".into()))?;
    db.delete_auto_task(name)?;
    Ok(json!({ "ok": true }))
}

fn handle_context(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let session_id = params["session_id"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'session_id' parameter".into()))?;
    let sequence = params["sequence"]
        .as_u64()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'sequence' parameter".into()))?;
    let count = params["count"].as_u64().unwrap_or(3) as u32;
    let (before, after) = db.get_surrounding_events(session_id, sequence, count)?;
    Ok(json!({ "before": before, "after": after }))
}

fn handle_privacy_mode_get(config: &StenoConfig) -> steno_core::Result<Value> {
    Ok(json!({ "private": config.private_mode }))
}

fn handle_privacy_rules_list(db: &Database) -> steno_core::Result<Value> {
    let mut stmt = db
        .conn
        .prepare("SELECT id, pattern, replacement, scope, enabled, builtin FROM redaction_rules")?;
    let redaction: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "pattern": row.get::<_, String>(1)?,
                "replacement": row.get::<_, String>(2)?,
                "scope": row.get::<_, String>(3)?,
                "enabled": row.get::<_, bool>(4)?,
                "builtin": row.get::<_, bool>(5)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = db
        .conn
        .prepare("SELECT id, pattern, match_type, enabled FROM ignore_rules")?;
    let ignore: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "pattern": row.get::<_, String>(1)?,
                "match_type": row.get::<_, String>(2)?,
                "enabled": row.get::<_, bool>(3)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(json!({ "redaction": redaction, "ignore": ignore }))
}

fn handle_privacy_rules_add(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let pattern = params["pattern"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'pattern'".into()))?;
    let rule_type = params["type"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'type'".into()))?;

    match rule_type {
        "redaction" => {
            let scope = params["scope"].as_str().unwrap_or("command");
            let replacement = params["replacement"].as_str().unwrap_or("[REDACTED]");
            db.conn.execute(
                "INSERT INTO redaction_rules (pattern, replacement, scope) VALUES (?1, ?2, ?3)",
                rusqlite::params![pattern, replacement, scope],
            )?;
            let id = db.conn.last_insert_rowid();
            Ok(json!({ "rule": { "id": id, "type": "redaction", "pattern": pattern } }))
        }
        "ignore" => {
            let match_type = params["match_type"].as_str().unwrap_or("glob");
            db.conn.execute(
                "INSERT INTO ignore_rules (pattern, match_type) VALUES (?1, ?2)",
                rusqlite::params![pattern, match_type],
            )?;
            let id = db.conn.last_insert_rowid();
            Ok(json!({ "rule": { "id": id, "type": "ignore", "pattern": pattern } }))
        }
        _ => Err(steno_core::Error::Ipc(format!(
            "Invalid rule type: {}",
            rule_type
        ))),
    }
}

fn handle_privacy_rules_remove(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let id = params["id"]
        .as_i64()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'id'".into()))?;
    let rule_type = params["type"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'type'".into()))?;

    match rule_type {
        "redaction" => {
            db.conn.execute(
                "DELETE FROM redaction_rules WHERE id = ?1 AND builtin = 0",
                [id],
            )?;
        }
        "ignore" => {
            db.conn
                .execute("DELETE FROM ignore_rules WHERE id = ?1", [id])?;
        }
        _ => {
            return Err(steno_core::Error::Ipc(format!(
                "Invalid rule type: {}",
                rule_type
            )));
        }
    }

    Ok(json!({ "ok": true }))
}

fn handle_privacy_redact(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let since = params["since"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'since'".into()))?;
    let until = params["until"]
        .as_str()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'until'".into()))?;

    let mut stmt = db
        .conn
        .prepare("SELECT id, command FROM events WHERE started_at >= ?1 AND started_at <= ?2")?;
    let events: Vec<(i64, String)> = stmt
        .query_map(rusqlite::params![since, until], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut count = 0u64;
    for (id, _cmd) in &events {
        db.conn
            .execute("UPDATE events SET redacted = 1 WHERE id = ?1", [id])?;
        count += 1;
    }

    Ok(json!({ "count": count }))
}

fn handle_export_json(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let session = params["session"].as_str();
    let since = params["since"].as_str();
    let until = params["until"].as_str();

    let events = if let Some(sid) = session {
        db.get_session_events(sid)?
    } else {
        let limit = 10000i64;
        if let (Some(s), Some(u)) = (since, until) {
            db.search_events("*", None, None, Some(s), Some(u), None, limit)?
        } else {
            db.tail_events(None, limit)?
        }
    };

    Ok(json!({ "data": events }))
}

fn handle_import_events(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let shell = params["shell"].as_str().unwrap_or("unknown").to_string();
    let commands = params["commands"]
        .as_array()
        .ok_or_else(|| steno_core::Error::Ipc("Missing 'commands' array".into()))?;

    if commands.is_empty() {
        return Ok(json!({ "imported": 0, "session_id": "" }));
    }

    let ts = chrono::Utc::now();
    let session_id = format!("primed-{}", ts.timestamp_millis());
    let started_at = ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let home_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());

    let session = steno_core::model::Session {
        id: session_id.clone(),
        display_id: 0,
        shell: shell.clone(),
        pid: 0,
        ppid: None,
        tty: None,
        hostname: "primed".to_string(),
        username: std::env::var("USER").unwrap_or_else(|_| "unknown".to_string()),
        cwd_start: home_dir.clone(),
        started_at: started_at.clone(),
        ended_at: Some(started_at.clone()),
        exit_code: None,
        tmux_pane: None,
        ssh_conn: None,
        metadata: Some(r#"{"source":"prime"}"#.to_string()),
    };

    db.insert_session(&session)?;

    let mut imported = 0u64;
    for (i, cmd_obj) in commands.iter().enumerate() {
        let command = match cmd_obj["command"].as_str() {
            Some(c) if !c.trim().is_empty() => c,
            _ => continue,
        };
        let cwd = cmd_obj["cwd"].as_str().unwrap_or(home_dir.as_str());
        let seq = (i + 1) as u64;

        let event_id = db.insert_event(
            &session_id,
            seq,
            command,
            None,
            cwd,
            None,
            None,
            None,
            &started_at,
            false,
            false,
        )?;

        if event_id > 0 {
            let _ = db.complete_event(&session_id, seq, 0, &started_at);
            imported += 1;
        }
    }

    Ok(json!({ "imported": imported, "session_id": session_id }))
}

fn handle_gc_run(db: &Database, params: &Value) -> steno_core::Result<Value> {
    let older_than = params["older_than"]
        .as_str()
        .unwrap_or("1970-01-01T00:00:00Z");

    let cutoff = if older_than == "1970-01-01T00:00:00Z" {
        let days = params["older_than_days"].as_u64().unwrap_or(90);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
        cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    } else {
        older_than.to_string()
    };

    let dry_run = params["dry_run"].as_bool().unwrap_or(false);
    let (deleted_events, deleted_sessions, freed_bytes) = db.gc_events(&cutoff, dry_run)?;

    Ok(json!({
        "deleted_events": deleted_events,
        "deleted_sessions": deleted_sessions,
        "freed_bytes": freed_bytes,
    }))
}
