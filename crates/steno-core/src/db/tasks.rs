use std::collections::HashMap;

use rusqlite::params;

use crate::model::task::{Task, TaskSource};
use crate::{Error, Result};

use super::Database;

impl Database {
    pub fn upsert_auto_task(
        &self,
        name: &str,
        description: Option<&str>,
        commands: &[String],
        confidence: f64,
    ) -> Result<i64> {
        let commands_json = serde_json::to_string(commands)?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let existing: Option<i64> = self
            .conn
            .query_row("SELECT id FROM tasks WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .ok();

        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE tasks SET commands_json = ?1, confidence = ?2,
                        occurrence_count = occurrence_count + 1, last_seen = ?3
                 WHERE id = ?4",
                params![commands_json, confidence, now, id],
            )?;
            Ok(id)
        } else {
            self.conn.execute(
                "INSERT INTO tasks (name, description, commands_json, source, confidence,
                        occurrence_count, last_seen, created_at)
                 VALUES (?1, ?2, ?3, 'auto', ?4, 1, ?5, ?5)",
                params![name, description, commands_json, confidence, now],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    pub fn list_auto_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, commands_json, source, confidence,
                    occurrence_count, last_seen, created_at
             FROM tasks
             ORDER BY occurrence_count DESC, confidence DESC",
        )?;
        let rows = stmt.query_map([], row_to_task)?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        Ok(tasks)
    }

    pub fn get_auto_task_by_name(&self, name: &str) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, name, description, commands_json, source, confidence,
                        occurrence_count, last_seen, created_at
                 FROM tasks WHERE name = ?1",
                [name],
                row_to_task,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::TaskNotFound(name.to_string()),
                other => other.into(),
            })
    }

    pub fn delete_auto_task(&self, name: &str) -> Result<()> {
        // Unlink events first
        let task_id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM tasks WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .ok();

        if let Some(id) = task_id {
            self.conn
                .execute("UPDATE events SET task_id = NULL WHERE task_id = ?1", [id])?;
        }

        let changed = self
            .conn
            .execute("DELETE FROM tasks WHERE name = ?1", [name])?;
        if changed == 0 {
            return Err(Error::TaskNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn get_task_events(&self, task_id: i64) -> Result<Vec<crate::model::StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, sequence, command, command_normalized,
                    cwd, canonical_cwd, cd_target_raw, cd_target_canonical,
                    started_at, ended_at, exit_code, duration_ms,
                    git_branch, git_root, task_id, redacted, incomplete
             FROM events WHERE task_id = ?1
             ORDER BY started_at ASC",
        )?;
        let rows = stmt.query_map([task_id], |row| {
            Ok(crate::model::StoredEvent {
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
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    pub fn get_task_by_name(&self, name: &str) -> Result<Task> {
        self.get_auto_task_by_name(name)
    }

    pub fn get_task_by_id(&self, id: i64) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, name, description, commands_json, source, confidence,
                        occurrence_count, last_seen, created_at
                 FROM tasks WHERE id = ?1",
                [id],
                row_to_task,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::TaskNotFound(id.to_string()),
                other => other.into(),
            })
    }
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let commands_json: String = row.get(3)?;
    let commands: Vec<String> = serde_json::from_str(&commands_json).unwrap_or_default();
    let source_str: String = row.get(4)?;
    let source = if source_str == "user" {
        TaskSource::User
    } else {
        TaskSource::Auto
    };

    Ok(Task {
        name: row.get(1)?,
        description: row.get(2)?,
        commands,
        vars: HashMap::new(),
        source,
        confidence: row.get(5)?,
        occurrence_count: row.get::<_, u32>(6)?,
        last_seen: row.get(7)?,
        db_id: Some(row.get(0)?),
    })
}
