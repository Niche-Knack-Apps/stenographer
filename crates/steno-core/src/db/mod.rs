pub mod events;
pub mod schema;
pub mod sessions;
pub mod tasks;

use std::path::Path;

use rusqlite::Connection;

use crate::Result;

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::Error::FileWrite {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let conn = Connection::open(path)?;

        // Enforce file permissions 0600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        // Set pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        let mut db = Self { conn };
        schema::initialize(&mut db)?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;",
        )?;
        let mut db = Self { conn };
        schema::initialize(&mut db)?;
        Ok(db)
    }
}
