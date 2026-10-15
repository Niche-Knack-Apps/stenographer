use rusqlite::params;

use crate::model::Session;
use crate::{Error, Result};

use super::Database;

impl Database {
    fn next_display_id(&self) -> Result<i64> {
        let max: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(display_id), 0) FROM sessions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(max + 1)
    }

    pub fn insert_session(&self, session: &Session) -> Result<()> {
        let display_id = if session.display_id > 0 {
            session.display_id
        } else {
            self.next_display_id()?
        };

        self.conn.execute(
            "INSERT OR IGNORE INTO sessions
             (id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start, started_at,
              ended_at, exit_code, tmux_pane, ssh_conn, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                session.id,
                display_id,
                session.shell,
                session.pid,
                session.ppid,
                session.tty,
                session.hostname,
                session.username,
                session.cwd_start,
                session.started_at,
                session.ended_at,
                session.exit_code,
                session.tmux_pane,
                session.ssh_conn,
                session.metadata,
            ],
        )?;
        Ok(())
    }

    pub fn end_session(&self, id: &str, ended_at: &str, exit_code: Option<i32>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1, exit_code = ?2 WHERE id = ?3 AND ended_at IS NULL",
            params![ended_at, exit_code, id],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        self.conn
            .query_row(
                "SELECT id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start,
                        started_at, ended_at, exit_code, tmux_pane, ssh_conn, metadata
                 FROM sessions WHERE id = ?1",
                [id],
                row_to_session,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::SessionNotFound(id.to_string()),
                other => other.into(),
            })
    }

    pub fn get_session_by_display_id(&self, display_id: i64) -> Result<Session> {
        self.conn
            .query_row(
                "SELECT id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start,
                        started_at, ended_at, exit_code, tmux_pane, ssh_conn, metadata
                 FROM sessions WHERE display_id = ?1",
                [display_id],
                row_to_session,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::SessionNotFound(display_id.to_string())
                }
                other => other.into(),
            })
    }

    /// Resolve a session identifier: numeric → display_id lookup, otherwise UUID prefix match.
    pub fn resolve_session(&self, id_str: &str) -> Result<Session> {
        if let Ok(display_id) = id_str.parse::<i64>() {
            return self.get_session_by_display_id(display_id);
        }
        // Try UUID prefix match
        self.conn
            .query_row(
                "SELECT id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start,
                        started_at, ended_at, exit_code, tmux_pane, ssh_conn, metadata
                 FROM sessions WHERE id LIKE ?1 || '%'
                 ORDER BY started_at DESC LIMIT 1",
                [id_str],
                row_to_session,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::SessionNotFound(id_str.to_string()),
                other => other.into(),
            })
    }

    pub fn list_sessions(
        &self,
        active_only: bool,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Session>> {
        let sql = if active_only {
            "SELECT id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start,
                    started_at, ended_at, exit_code, tmux_pane, ssh_conn, metadata
             FROM sessions WHERE ended_at IS NULL
             ORDER BY started_at DESC LIMIT ?1 OFFSET ?2"
        } else {
            "SELECT id, display_id, shell, pid, ppid, tty, hostname, username, cwd_start,
                    started_at, ended_at, exit_code, tmux_pane, ssh_conn, metadata
             FROM sessions
             ORDER BY started_at DESC LIMIT ?1 OFFSET ?2"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit, offset], row_to_session)?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn get_active_session_pids(&self) -> Result<Vec<(String, u32)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, pid FROM sessions WHERE ended_at IS NULL")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        display_id: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
        shell: row.get(2)?,
        pid: row.get(3)?,
        ppid: row.get(4)?,
        tty: row.get(5)?,
        hostname: row.get(6)?,
        username: row.get(7)?,
        cwd_start: row.get(8)?,
        started_at: row.get(9)?,
        ended_at: row.get(10)?,
        exit_code: row.get(11)?,
        tmux_pane: row.get(12)?,
        ssh_conn: row.get(13)?,
        metadata: row.get(14)?,
    })
}
