use crate::Result;

use super::Database;

const CURRENT_VERSION: i32 = 5;

pub fn initialize(db: &mut Database) -> Result<()> {
    // Check schema version BEFORE running SCHEMA so we know if this is
    // an existing database that needs migrations first.
    let version = get_version(db)?;

    if version == 0 {
        // Fresh database — create everything at once.
        db.conn.execute_batch(SCHEMA)?;
        db.conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
            [CURRENT_VERSION],
        )?;
        seed_builtin_rules(db)?;
    } else {
        // Existing database — run migrations before the static schema
        // so that columns exist before indexes reference them.
        if version < 2 {
            migrate_v1_to_v2(db)?;
        }
        if version < 3 {
            migrate_v2_to_v3(db)?;
        }
        if version < 4 {
            migrate_v3_to_v4(db)?;
        }
        // Now safe to run the full schema (CREATE IF NOT EXISTS is idempotent).
        db.conn.execute_batch(SCHEMA)?;
        // Data-only backfill — run after SCHEMA so the FTS table/triggers exist.
        if version < 5 {
            migrate_v4_to_v5(db)?;
        }
    }

    Ok(())
}

fn migrate_v1_to_v2(db: &mut Database) -> Result<()> {
    db.conn
        .execute_batch("ALTER TABLE tasks ADD COLUMN auto_group INTEGER NOT NULL DEFAULT 0")?;
    db.conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [2],
    )?;
    Ok(())
}

fn migrate_v2_to_v3(db: &mut Database) -> Result<()> {
    // Add display_id to sessions
    db.conn
        .execute_batch("ALTER TABLE sessions ADD COLUMN display_id INTEGER")?;

    // Populate display_id for existing sessions using rowid order
    db.conn.execute_batch(
        "UPDATE sessions SET display_id = (
            SELECT COUNT(*) FROM sessions s2 WHERE s2.rowid <= sessions.rowid
        )",
    )?;

    // Clear old task references (incompatible schema change)
    db.conn.execute_batch("UPDATE events SET task_id = NULL")?;

    // Recreate tasks table with new schema
    db.conn.execute_batch("DROP TABLE IF EXISTS tasks")?;
    db.conn.execute_batch(
        "CREATE TABLE tasks (
            id               INTEGER PRIMARY KEY,
            name             TEXT NOT NULL UNIQUE,
            description      TEXT,
            commands_json    TEXT NOT NULL DEFAULT '[]',
            source           TEXT NOT NULL DEFAULT 'auto',
            confidence       REAL NOT NULL DEFAULT 0.0,
            occurrence_count INTEGER NOT NULL DEFAULT 0,
            last_seen        TEXT,
            created_at       TEXT NOT NULL
        )",
    )?;

    // Drop unused tables
    db.conn.execute_batch("DROP TABLE IF EXISTS moments")?;
    db.conn.execute_batch("DROP TABLE IF EXISTS workflows")?;

    db.conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [3],
    )?;
    Ok(())
}

fn migrate_v3_to_v4(db: &mut Database) -> Result<()> {
    db.conn.execute_batch(
        "ALTER TABLE events ADD COLUMN canonical_cwd TEXT;
         ALTER TABLE events ADD COLUMN cd_target_raw TEXT;
         ALTER TABLE events ADD COLUMN cd_target_canonical TEXT;
         CREATE INDEX IF NOT EXISTS idx_events_dedup ON events(command, canonical_cwd);",
    )?;
    db.conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [4],
    )?;
    Ok(())
}

/// Backfill: rewrite resolvable relative paths in stored commands to absolute
/// paths rooted at each row's recorded cwd. The `events_au` UPDATE trigger
/// re-syncs `events_fts` automatically.
///
/// Existence is checked against the CURRENT filesystem — historical filesystem
/// state isn't recorded — so rows whose files have since moved/been deleted
/// stay relative, and a relative token that didn't exist then but matches a
/// file in that cwd now may be rewritten. This is inherent to backfilling and
/// is accepted; ingest-time rewriting does not have this limitation.
///
/// A single collect-then-update full-table pass is fine for the expected DB
/// size (history is bounded by retention). Chunking is not implemented.
fn migrate_v4_to_v5(db: &mut Database) -> Result<()> {
    let rows: Vec<(i64, String, String)> = {
        let mut stmt = db.conn.prepare("SELECT id, command, cwd FROM events")?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        mapped.filter_map(|r| r.ok()).collect()
    };

    let total = rows.len();
    let mut rewritten = 0usize;
    for (id, command, cwd) in rows {
        let abs = crate::normalize::absolutize_command_paths(&command, &cwd);
        if abs != command {
            let normalized = crate::normalize::normalize_command(&abs);
            db.conn.execute(
                "UPDATE events SET command = ?1, command_normalized = ?2 WHERE id = ?3",
                rusqlite::params![abs, normalized, id],
            )?;
            rewritten += 1;
        }
    }
    tracing::info!("path backfill: rewrote {rewritten} of {total} events");

    db.conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [5],
    )?;
    Ok(())
}

fn get_version(db: &Database) -> Result<i32> {
    // Check if schema_version table exists first (fresh DB won't have it)
    let table_exists: bool = db.conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        return Ok(0);
    }

    let result = db.conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    );

    match result {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

fn seed_builtin_rules(db: &mut Database) -> Result<()> {
    let rules = [
        (r"pass(word)?[\s=:].*", "[REDACTED]", "command"),
        (
            r"(export\s+)?\w*(TOKEN|SECRET|KEY|PASSWORD|PASSWD|API_KEY)\w*\s*=\s*\S+",
            "[REDACTED]",
            "command",
        ),
        (r"sshpass\s+.*", "sshpass [REDACTED]", "command"),
        (
            r"(aws\s+configure|op\s+signin|vault\s+login)\b.*",
            "$1 [REDACTED]",
            "command",
        ),
        (
            r"(mysql|psql|mongo)\s+.*-p\s*\S+",
            "[REDACTED PASSWORD]",
            "command",
        ),
        (
            r"(curl|wget)\s+.*(-u|--user)\s+\S+",
            "[REDACTED CREDS]",
            "command",
        ),
    ];

    for (pattern, replacement, scope) in &rules {
        db.conn.execute(
            "INSERT OR IGNORE INTO redaction_rules (pattern, replacement, scope, enabled, builtin)
             VALUES (?1, ?2, ?3, 1, 1)",
            rusqlite::params![pattern, replacement, scope],
        )?;
    }

    Ok(())
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    display_id  INTEGER,
    shell       TEXT NOT NULL,
    pid         INTEGER NOT NULL,
    ppid        INTEGER,
    tty         TEXT,
    hostname    TEXT NOT NULL,
    username    TEXT NOT NULL,
    cwd_start   TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    exit_code   INTEGER,
    tmux_pane   TEXT,
    ssh_conn    TEXT,
    metadata    TEXT
);

CREATE TABLE IF NOT EXISTS tasks (
    id               INTEGER PRIMARY KEY,
    name             TEXT NOT NULL UNIQUE,
    description      TEXT,
    commands_json    TEXT NOT NULL DEFAULT '[]',
    source           TEXT NOT NULL DEFAULT 'auto',
    confidence       REAL NOT NULL DEFAULT 0.0,
    occurrence_count INTEGER NOT NULL DEFAULT 0,
    last_seen        TEXT,
    created_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    sequence    INTEGER NOT NULL,
    command     TEXT NOT NULL,
    command_normalized TEXT,
    cwd         TEXT NOT NULL,
    canonical_cwd TEXT,
    cd_target_raw TEXT,
    cd_target_canonical TEXT,
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    exit_code   INTEGER,
    duration_ms INTEGER,
    git_branch  TEXT,
    git_root    TEXT,
    task_id     INTEGER REFERENCES tasks(id),
    redacted    INTEGER NOT NULL DEFAULT 0,
    incomplete  INTEGER NOT NULL DEFAULT 0,
    UNIQUE(session_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, sequence);
CREATE INDEX IF NOT EXISTS idx_events_time    ON events(started_at);
CREATE INDEX IF NOT EXISTS idx_events_cwd     ON events(cwd);
CREATE INDEX IF NOT EXISTS idx_events_task    ON events(task_id) WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_events_dedup  ON events(command, canonical_cwd);

CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
    command, cwd,
    content='events', content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS events_ai AFTER INSERT ON events BEGIN
    INSERT INTO events_fts(rowid, command, cwd) VALUES (new.id, new.command, new.cwd);
END;
CREATE TRIGGER IF NOT EXISTS events_ad AFTER DELETE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, command, cwd) VALUES ('delete', old.id, old.command, old.cwd);
END;
CREATE TRIGGER IF NOT EXISTS events_au AFTER UPDATE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, command, cwd) VALUES ('delete', old.id, old.command, old.cwd);
    INSERT INTO events_fts(rowid, command, cwd) VALUES (new.id, new.command, new.cwd);
END;

CREATE TABLE IF NOT EXISTS redaction_rules (
    id          INTEGER PRIMARY KEY,
    pattern     TEXT NOT NULL,
    replacement TEXT NOT NULL DEFAULT '[REDACTED]',
    scope       TEXT NOT NULL DEFAULT 'command',
    enabled     INTEGER NOT NULL DEFAULT 1,
    builtin     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS ignore_rules (
    id          INTEGER PRIMARY KEY,
    pattern     TEXT NOT NULL,
    match_type  TEXT NOT NULL DEFAULT 'glob',
    enabled     INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER NOT NULL,
    applied_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS spool_offset (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    byte_offset INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO spool_offset (id, byte_offset) VALUES (1, 0);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Session;
    use std::path::PathBuf;

    fn temp_cwd(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("steno_sch_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed_session(db: &Database, cwd: &str) {
        db.insert_session(&Session {
            id: "s".to_string(),
            display_id: 1,
            shell: "zsh".to_string(),
            pid: 1,
            ppid: None,
            tty: None,
            hostname: "h".to_string(),
            username: "u".to_string(),
            cwd_start: cwd.to_string(),
            started_at: "2025-01-01T00:00:00.000Z".to_string(),
            ended_at: None,
            exit_code: None,
            tmux_pane: None,
            ssh_conn: None,
            metadata: None,
        })
        .unwrap();
    }

    #[test]
    fn test_v5_backfill_rewrites_and_syncs_fts() {
        let cwd = temp_cwd("backfill");
        std::fs::create_dir_all(cwd.join("sub")).unwrap();
        std::fs::write(cwd.join("sub/notes.md"), b"").unwrap();
        let cwd_str = cwd.to_str().unwrap();

        // Fresh in-memory DB is already at the current schema (FTS + triggers
        // exist). Insert rows with relative commands directly to simulate a
        // pre-v5 database, then run the backfill migration.
        let mut db = Database::open_in_memory().unwrap();
        seed_session(&db, cwd_str);

        let rel = "cat sub/notes.md";
        let id1 = db
            .insert_event(
                "s",
                0,
                rel,
                Some(rel),
                cwd_str,
                None,
                None,
                None,
                "2025-01-01T00:00:01.000Z",
                false,
                false,
            )
            .unwrap();
        let ghost = "cat ghost/missing.md";
        let id2 = db
            .insert_event(
                "s",
                1,
                ghost,
                Some(ghost),
                cwd_str,
                None,
                None,
                None,
                "2025-01-01T00:00:02.000Z",
                false,
                false,
            )
            .unwrap();

        migrate_v4_to_v5(&mut db).unwrap();

        let expected = format!("cat {}/sub/notes.md", cwd.display());

        // Resolvable relative path is rewritten in place.
        let cmd1: String = db
            .conn
            .query_row("SELECT command FROM events WHERE id = ?1", [id1], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cmd1, expected);

        // Non-resolving relative path is left untouched.
        let cmd2: String = db
            .conn
            .query_row("SELECT command FROM events WHERE id = ?1", [id2], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cmd2, ghost);

        // The events_au UPDATE trigger re-synced events_fts to the new command.
        let fts1: String = db
            .conn
            .query_row(
                "SELECT command FROM events_fts WHERE rowid = ?1",
                [id1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts1, expected);

        std::fs::remove_dir_all(&cwd).ok();
    }
}
