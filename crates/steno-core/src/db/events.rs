use rusqlite::params;

use crate::Result;
use crate::model::StoredEvent;

use super::Database;

/// Normalize a user-facing limit for SQLite. SQLite interprets `LIMIT -1` as
/// "no limit", so 0 or any negative value collapses to -1 (unlimited); positive
/// values pass through unchanged.
fn normalize_limit(limit: i64) -> i64 {
    if limit <= 0 { -1 } else { limit }
}

impl Database {
    #[allow(clippy::too_many_arguments)]
    pub fn insert_event(
        &self,
        session_id: &str,
        sequence: u64,
        command: &str,
        command_normalized: Option<&str>,
        cwd: &str,
        canonical_cwd: Option<&str>,
        cd_target_raw: Option<&str>,
        cd_target_canonical: Option<&str>,
        started_at: &str,
        redacted: bool,
        incomplete: bool,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO events
             (session_id, sequence, command, command_normalized, cwd, canonical_cwd,
              cd_target_raw, cd_target_canonical, started_at, redacted, incomplete)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                session_id,
                sequence,
                command,
                command_normalized,
                cwd,
                canonical_cwd,
                cd_target_raw,
                cd_target_canonical,
                started_at,
                redacted as i32,
                incomplete as i32,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn complete_event(
        &self,
        session_id: &str,
        sequence: u64,
        exit_code: i32,
        ended_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE events SET exit_code = ?1, ended_at = ?2,
                    duration_ms = CAST(
                        (julianday(?2) - julianday(started_at)) * 86400000 AS INTEGER
                    ),
                    incomplete = 0
             WHERE session_id = ?3 AND sequence = ?4",
            params![exit_code, ended_at, session_id, sequence],
        )?;
        Ok(())
    }

    pub fn enrich_event(
        &self,
        event_id: i64,
        git_branch: Option<&str>,
        git_root: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE events SET git_branch = ?1, git_root = ?2 WHERE id = ?3",
            params![git_branch, git_root, event_id],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_events(
        &self,
        pattern: &str,
        cwd: Option<&str>,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        exit_code: Option<i32>,
        limit: i64,
    ) -> Result<Vec<StoredEvent>> {
        // SQLite treats LIMIT -1 as unlimited; map 0/negatives to that.
        let limit = normalize_limit(limit);
        let mut conditions =
            vec!["e.id IN (SELECT rowid FROM events_fts WHERE events_fts MATCH ?1)".to_string()];
        let mut param_idx = 2u32;

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(pattern.to_string())];

        if let Some(c) = cwd {
            conditions.push(format!("e.cwd LIKE ?{param_idx}"));
            param_values.push(Box::new(format!("{c}%")));
            param_idx += 1;
        }

        if let Some(s) = session_id {
            conditions.push(format!("e.session_id = ?{param_idx}"));
            param_values.push(Box::new(s.to_string()));
            param_idx += 1;
        }

        if let Some(s) = since {
            conditions.push(format!("e.started_at >= ?{param_idx}"));
            param_values.push(Box::new(s.to_string()));
            param_idx += 1;
        }

        if let Some(u) = until {
            conditions.push(format!("e.started_at <= ?{param_idx}"));
            param_values.push(Box::new(u.to_string()));
            param_idx += 1;
        }

        if let Some(ec) = exit_code {
            conditions.push(format!("e.exit_code = ?{param_idx}"));
            param_values.push(Box::new(ec));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT e.id, e.session_id, e.sequence, e.command, e.command_normalized,
                    e.cwd, e.canonical_cwd, e.cd_target_raw, e.cd_target_canonical,
                    e.started_at, e.ended_at, e.exit_code, e.duration_ms,
                    e.git_branch, e.git_root, e.task_id, e.redacted, e.incomplete
             FROM events e
             WHERE {where_clause}
             ORDER BY e.started_at DESC
             LIMIT ?{param_idx}"
        );
        param_values.push(Box::new(limit));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_event)?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn tail_events(&self, session_id: Option<&str>, limit: i64) -> Result<Vec<StoredEvent>> {
        let limit = normalize_limit(limit);
        let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(sid) = session_id {
                (
                    "SELECT id, session_id, sequence, command, command_normalized,
                        cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                        started_at, ended_at, exit_code, duration_ms,
                        git_branch, git_root, task_id, redacted, incomplete
                 FROM events WHERE session_id = ?1
                 ORDER BY started_at DESC LIMIT ?2"
                        .to_string(),
                    vec![Box::new(sid.to_string()), Box::new(limit)],
                )
            } else {
                (
                    "SELECT id, session_id, sequence, command, command_normalized,
                        cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                        started_at, ended_at, exit_code, duration_ms,
                        git_branch, git_root, task_id, redacted, incomplete
                 FROM events
                 ORDER BY started_at DESC LIMIT ?1"
                        .to_string(),
                    vec![Box::new(limit)],
                )
            };

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_event)?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn get_session_events(&self, session_id: &str) -> Result<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, sequence, command, command_normalized,
                    cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                    started_at, ended_at, exit_code, duration_ms,
                    git_branch, git_root, task_id, redacted, incomplete
             FROM events WHERE session_id = ?1
             ORDER BY sequence ASC",
        )?;
        let rows = stmt.query_map([session_id], row_to_event)?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn get_events_count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn get_events_needing_enrichment(&self, limit: u32) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, cwd FROM events
             WHERE git_branch IS NULL AND incomplete = 0
             ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn attach_task_to_events(&self, task_id: i64, event_ids: &[i64]) -> Result<u32> {
        let mut count = 0u32;
        for id in event_ids {
            let changed = self.conn.execute(
                "UPDATE events SET task_id = ?1 WHERE id = ?2",
                params![task_id, id],
            )?;
            count += changed as u32;
        }
        Ok(count)
    }

    pub fn get_surrounding_events(
        &self,
        session_id: &str,
        sequence: u64,
        count: u32,
    ) -> Result<(Vec<StoredEvent>, Vec<StoredEvent>)> {
        let mut stmt_before = self.conn.prepare(
            "SELECT id, session_id, sequence, command, command_normalized,
                    cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                    started_at, ended_at, exit_code, duration_ms,
                    git_branch, git_root, task_id, redacted, incomplete
             FROM events WHERE session_id = ?1 AND sequence < ?2
             ORDER BY sequence DESC LIMIT ?3",
        )?;
        let before_rows =
            stmt_before.query_map(params![session_id, sequence, count], row_to_event)?;
        let mut before: Vec<StoredEvent> = Vec::new();
        for row in before_rows {
            before.push(row?);
        }
        before.reverse();

        let mut stmt_after = self.conn.prepare(
            "SELECT id, session_id, sequence, command, command_normalized,
                    cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                    started_at, ended_at, exit_code, duration_ms,
                    git_branch, git_root, task_id, redacted, incomplete
             FROM events WHERE session_id = ?1 AND sequence > ?2
             ORDER BY sequence ASC LIMIT ?3",
        )?;
        let after_rows =
            stmt_after.query_map(params![session_id, sequence, count], row_to_event)?;
        let mut after: Vec<StoredEvent> = Vec::new();
        for row in after_rows {
            after.push(row?);
        }

        Ok((before, after))
    }

    pub fn get_ungrouped_events(&self, limit: u32) -> Result<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, sequence, command, command_normalized,
                    cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                    started_at, ended_at, exit_code, duration_ms,
                    git_branch, git_root, task_id, redacted, incomplete
             FROM events
             WHERE task_id IS NULL AND incomplete = 0
             ORDER BY started_at ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], row_to_event)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn tail_events_dedup(
        &self,
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: i64,
    ) -> Result<Vec<StoredEvent>> {
        let limit = normalize_limit(limit);
        let mut clauses: Vec<String> = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(sid) = session_id {
            param_values.push(Box::new(sid.to_string()));
            clauses.push(format!("session_id = ?{}", param_values.len()));
        }
        if let Some(c) = cwd {
            param_values.push(Box::new(format!("{c}%")));
            clauses.push(format!("cwd LIKE ?{}", param_values.len()));
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };

        let limit_param = param_values.len() + 1;
        let sql = format!(
            "SELECT * FROM (
                SELECT id, session_id, sequence, command, command_normalized,
                       cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                       started_at, ended_at, exit_code, duration_ms,
                       git_branch, git_root, task_id, redacted, incomplete,
                       ROW_NUMBER() OVER (
                           PARTITION BY command, COALESCE(canonical_cwd, cwd)
                           ORDER BY started_at DESC
                       ) AS rn,
                       COUNT(*) OVER (
                           PARTITION BY command, COALESCE(canonical_cwd, cwd)
                       ) AS occ_count
                FROM events
                {where_clause}
            ) sub
            WHERE rn = 1
            ORDER BY started_at DESC
            LIMIT ?{limit_param}"
        );

        let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = param_values;
        all_params.push(Box::new(limit));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_event_dedup)?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_events_dedup(
        &self,
        pattern: &str,
        cwd: Option<&str>,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        exit_code: Option<i32>,
        limit: i64,
    ) -> Result<Vec<StoredEvent>> {
        let limit = normalize_limit(limit);
        let mut conditions =
            vec!["e.id IN (SELECT rowid FROM events_fts WHERE events_fts MATCH ?1)".to_string()];
        let mut param_idx = 2u32;

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(pattern.to_string())];

        if let Some(c) = cwd {
            conditions.push(format!("e.cwd LIKE ?{param_idx}"));
            param_values.push(Box::new(format!("{c}%")));
            param_idx += 1;
        }

        if let Some(s) = session_id {
            conditions.push(format!("e.session_id = ?{param_idx}"));
            param_values.push(Box::new(s.to_string()));
            param_idx += 1;
        }

        if let Some(s) = since {
            conditions.push(format!("e.started_at >= ?{param_idx}"));
            param_values.push(Box::new(s.to_string()));
            param_idx += 1;
        }

        if let Some(u) = until {
            conditions.push(format!("e.started_at <= ?{param_idx}"));
            param_values.push(Box::new(u.to_string()));
            param_idx += 1;
        }

        if let Some(ec) = exit_code {
            conditions.push(format!("e.exit_code = ?{param_idx}"));
            param_values.push(Box::new(ec));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT * FROM (
                SELECT e.id, e.session_id, e.sequence, e.command, e.command_normalized,
                       e.cwd, e.canonical_cwd, e.cd_target_raw, e.cd_target_canonical,
                       e.started_at, e.ended_at, e.exit_code, e.duration_ms,
                       e.git_branch, e.git_root, e.task_id, e.redacted, e.incomplete,
                       ROW_NUMBER() OVER (
                           PARTITION BY e.command, COALESCE(e.canonical_cwd, e.cwd)
                           ORDER BY e.started_at DESC
                       ) AS rn,
                       COUNT(*) OVER (
                           PARTITION BY e.command, COALESCE(e.canonical_cwd, e.cwd)
                       ) AS occ_count
                FROM events e
                WHERE {where_clause}
            ) sub
            WHERE rn = 1
            ORDER BY started_at DESC
            LIMIT ?{param_idx}"
        );
        param_values.push(Box::new(limit));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row_to_event_dedup)?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn get_spool_offset(&self) -> Result<u64> {
        let offset: i64 = self.conn.query_row(
            "SELECT byte_offset FROM spool_offset WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(offset as u64)
    }

    pub fn set_spool_offset(&self, offset: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE spool_offset SET byte_offset = ?1 WHERE id = 1",
            [offset as i64],
        )?;
        Ok(())
    }

    pub fn gc_events(&self, before: &str, dry_run: bool) -> Result<(u64, u64, u64)> {
        if dry_run {
            let event_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM events WHERE started_at < ?1",
                [before],
                |row| row.get(0),
            )?;
            let session_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE started_at < ?1 AND ended_at IS NOT NULL",
                [before],
                |row| row.get(0),
            )?;
            return Ok((event_count as u64, session_count as u64, 0));
        }

        let page_count_before: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;

        // Delete events first (FTS triggers handle cleanup)
        let events_deleted =
            self.conn
                .execute("DELETE FROM events WHERE started_at < ?1", [before])? as u64;

        // Delete sessions that are closed and have no remaining events
        let sessions_deleted = self.conn.execute(
            "DELETE FROM sessions WHERE started_at < ?1 AND ended_at IS NOT NULL
             AND id NOT IN (SELECT DISTINCT session_id FROM events)",
            [before],
        )? as u64;

        // Delete orphaned auto-tasks
        self.conn.execute(
            "DELETE FROM tasks WHERE id NOT IN (SELECT DISTINCT task_id FROM events WHERE task_id IS NOT NULL)",
            [],
        )?;

        self.conn.execute_batch("VACUUM")?;

        let page_count_after: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let freed = ((page_count_before - page_count_after) * page_size).max(0) as u64;

        Ok((events_deleted, sessions_deleted, freed))
    }

    pub fn db_size_bytes(&self) -> Result<u64> {
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok((page_count * page_size) as u64)
    }

    pub fn get_recent_normalized_commands(&self, limit: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT command_normalized FROM events
             WHERE command_normalized IS NOT NULL
             ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| row.get::<_, String>(0))?;

        let mut commands = Vec::new();
        for row in rows {
            commands.push(row?);
        }
        Ok(commands)
    }

    pub fn get_session_display_id(&self, session_id: &str) -> Result<i64> {
        let display_id: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(display_id, 0) FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(display_id)
    }
}

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    Ok(StoredEvent {
        id: row.get(0)?,
        session_id: row.get(1)?,
        sequence: row.get(2)?,
        command: row.get(3)?,
        command_normalized: row.get(4)?,
        cwd: row.get(5)?,
        canonical_cwd: row.get(6)?,
        cd_target_raw: row.get(7)?,
        cd_target_canonical: row.get(8)?,
        started_at: row.get(9)?,
        ended_at: row.get(10)?,
        exit_code: row.get(11)?,
        duration_ms: row.get(12)?,
        git_branch: row.get(13)?,
        git_root: row.get(14)?,
        task_id: row.get(15)?,
        redacted: row.get::<_, i32>(16)? != 0,
        incomplete: row.get::<_, i32>(17)? != 0,
        occurrence_count: None,
    })
}

fn row_to_event_dedup(row: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    let mut event = StoredEvent {
        id: row.get(0)?,
        session_id: row.get(1)?,
        sequence: row.get(2)?,
        command: row.get(3)?,
        command_normalized: row.get(4)?,
        cwd: row.get(5)?,
        canonical_cwd: row.get(6)?,
        cd_target_raw: row.get(7)?,
        cd_target_canonical: row.get(8)?,
        started_at: row.get(9)?,
        ended_at: row.get(10)?,
        exit_code: row.get(11)?,
        duration_ms: row.get(12)?,
        git_branch: row.get(13)?,
        git_root: row.get(14)?,
        task_id: row.get(15)?,
        redacted: row.get::<_, i32>(16)? != 0,
        incomplete: row.get::<_, i32>(17)? != 0,
        occurrence_count: None,
    };
    // rn at index 18, occ_count at index 19
    let occ: i64 = row.get(19)?;
    event.occurrence_count = Some(occ as u64);
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Session;

    fn setup_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            id: "test-session".to_string(),
            display_id: 1,
            shell: "zsh".to_string(),
            pid: 1000,
            ppid: Some(999),
            tty: None,
            hostname: "localhost".to_string(),
            username: "user".to_string(),
            cwd_start: "/home/user".to_string(),
            started_at: "2025-01-01T00:00:00.000Z".to_string(),
            ended_at: None,
            exit_code: None,
            tmux_pane: None,
            ssh_conn: None,
            metadata: None,
        })
        .unwrap();
        db
    }

    #[test]
    fn test_dedup_collapses_identical_commands() {
        let db = setup_db();
        for i in 0..3 {
            db.insert_event(
                "test-session",
                i,
                "ls",
                Some("ls"),
                "/home/user",
                None,
                None,
                None,
                &format!("2025-01-01T00:00:0{}.000Z", i),
                false,
                false,
            )
            .unwrap();
        }

        let all = db.tail_events(None, 10).unwrap();
        assert_eq!(all.len(), 3);

        let deduped = db.tail_events_dedup(None, None, 10).unwrap();
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].occurrence_count, Some(3));
        assert_eq!(deduped[0].started_at, "2025-01-01T00:00:02.000Z");
    }

    #[test]
    fn test_dedup_different_cwd_not_collapsed() {
        let db = setup_db();
        db.insert_event(
            "test-session",
            0,
            "ls",
            Some("ls"),
            "/home/user",
            None,
            None,
            None,
            "2025-01-01T00:00:00.000Z",
            false,
            false,
        )
        .unwrap();
        db.insert_event(
            "test-session",
            1,
            "ls",
            Some("ls"),
            "/tmp",
            None,
            None,
            None,
            "2025-01-01T00:00:01.000Z",
            false,
            false,
        )
        .unwrap();

        let deduped = db.tail_events_dedup(None, None, 10).unwrap();
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_dedup_latest_timestamp_wins() {
        let db = setup_db();
        for i in 0..3 {
            db.insert_event(
                "test-session",
                i,
                "make",
                Some("make"),
                "/project",
                None,
                None,
                None,
                &format!("2025-01-01T00:00:0{}.000Z", i),
                false,
                false,
            )
            .unwrap();
        }

        let deduped = db.tail_events_dedup(None, None, 10).unwrap();
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].started_at, "2025-01-01T00:00:02.000Z");
    }

    #[test]
    fn test_dedup_null_canonical_cwd_uses_raw() {
        let db = setup_db();
        // Two events: same command, same raw cwd, one with canonical_cwd, one without
        db.insert_event(
            "test-session",
            0,
            "ls",
            Some("ls"),
            "/home/user",
            Some("/home/user"),
            None,
            None,
            "2025-01-01T00:00:00.000Z",
            false,
            false,
        )
        .unwrap();
        db.insert_event(
            "test-session",
            1,
            "ls",
            Some("ls"),
            "/home/user",
            None,
            None,
            None,
            "2025-01-01T00:00:01.000Z",
            false,
            false,
        )
        .unwrap();

        // Both COALESCE to "/home/user" so they should be collapsed
        let deduped = db.tail_events_dedup(None, None, 10).unwrap();
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].occurrence_count, Some(2));
    }

    #[test]
    fn test_unlimited_limit_returns_all() {
        let db = setup_db();
        for i in 0..25 {
            db.insert_event(
                "test-session",
                i,
                &format!("cmd-{i}"),
                Some(&format!("cmd-{i}")),
                "/home/user",
                None,
                None,
                None,
                &format!("2025-01-01T00:00:{:02}.000Z", i),
                false,
                false,
            )
            .unwrap();
        }

        // Positive limit caps the result set.
        assert_eq!(db.tail_events(None, 10).unwrap().len(), 10);

        // 0 and -1 both mean "unlimited" — return everything.
        assert_eq!(db.tail_events(None, 0).unwrap().len(), 25);
        assert_eq!(db.tail_events(None, -1).unwrap().len(), 25);
        assert_eq!(db.tail_events_dedup(None, None, 0).unwrap().len(), 25);
        assert_eq!(
            db.search_events("cmd*", None, None, None, None, None, -1)
                .unwrap()
                .len(),
            25
        );
    }

    #[test]
    fn test_schema_v4_columns_exist() {
        let db = Database::open_in_memory().unwrap();
        // Verify the new columns exist by querying them
        let result = db.conn.execute_batch(
            "SELECT canonical_cwd, cd_target_raw, cd_target_canonical FROM events LIMIT 1",
        );
        assert!(result.is_ok());
    }
}
