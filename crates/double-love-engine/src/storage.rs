use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct ProjectStore {
    connection: Connection,
}

impl ProjectStore {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS project_meta (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS revisions (
                revision INTEGER PRIMARY KEY NOT NULL,
                operation TEXT NOT NULL,
                committed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS operation_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                revision INTEGER NOT NULL,
                operation TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                FOREIGN KEY (revision) REFERENCES revisions(revision)
            );
            INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
            ",
        )?;
        Ok(Self { connection })
    }

    pub fn project_id(&self) -> Result<Option<String>, StorageError> {
        self.connection
            .query_row(
                "SELECT value FROM project_meta WHERE key = 'project_id'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn set_project_id(&self, project_id: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO project_meta(key, value) VALUES('project_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![project_id],
        )?;
        Ok(())
    }

    pub fn revision(&self) -> Result<u64, StorageError> {
        self.connection
            .query_row(
                "SELECT COALESCE(MAX(revision), 0) FROM revisions",
                [],
                |row| row.get::<_, u64>(0),
            )
            .map_err(StorageError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::ProjectStore;

    #[test]
    fn creates_a_local_project_store_with_foreign_keys_and_wal() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("double-love-{unique}.sqlite"));

        let store = ProjectStore::open(&path).expect("store opens");
        store
            .set_project_id("test-project")
            .expect("project id writes");
        assert_eq!(
            store.project_id().expect("project id reads").as_deref(),
            Some("test-project")
        );
        assert_eq!(store.revision().expect("revision reads"), 0);
        drop(store);

        fs::remove_file(path).expect("temporary database is removed");
    }
}
