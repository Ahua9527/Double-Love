use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// v1：项目元信息 + 修订账本。
const MIGRATION_V1: &str = "
    CREATE TABLE project_meta (
        key TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    );
    CREATE TABLE revisions (
        revision INTEGER PRIMARY KEY NOT NULL,
        operation TEXT NOT NULL,
        committed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE operation_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        revision INTEGER NOT NULL,
        operation TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        FOREIGN KEY (revision) REFERENCES revisions(revision)
    );
";

/// v2：转录粗剪切片。媒体资产只读引用；TimelineIR 不落表（words+operations 每次现算）。
const MIGRATION_V2: &str = "
    CREATE TABLE media_asset (
        id TEXT PRIMARY KEY NOT NULL,
        kind TEXT NOT NULL,
        original_path TEXT NOT NULL UNIQUE,
        display_name TEXT NOT NULL,
        duration_samples INTEGER NOT NULL,
        audio_sample_rate INTEGER NOT NULL,
        fps_num INTEGER NOT NULL,
        fps_den INTEGER NOT NULL,
        video_timebase INTEGER NOT NULL,
        is_ntsc INTEGER NOT NULL,
        width INTEGER,
        height INTEGER,
        audio_channels INTEGER,
        source_tc_start_frame INTEGER,
        ffprobe_json TEXT NOT NULL,
        prepared_wav_path TEXT,
        status TEXT NOT NULL DEFAULT 'imported',
        imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE transcript_word (
        word_id TEXT PRIMARY KEY NOT NULL,
        asset_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL,
        raw_text TEXT NOT NULL,
        display_text TEXT NOT NULL,
        language TEXT,
        start_sample INTEGER NOT NULL,
        end_sample INTEGER NOT NULL,
        confidence REAL,
        synthetic INTEGER NOT NULL DEFAULT 0,
        source_word_ids_json TEXT,
        production_range_json TEXT,
        speaker_assignments_json TEXT,
        FOREIGN KEY (asset_id) REFERENCES media_asset(id),
        UNIQUE (asset_id, ordinal)
    );
    CREATE TABLE edit_operation (
        id TEXT PRIMARY KEY NOT NULL,
        revision INTEGER NOT NULL,
        asset_id TEXT NOT NULL,
        type TEXT NOT NULL,
        behavior TEXT NOT NULL,
        start_ordinal INTEGER NOT NULL,
        end_ordinal INTEGER NOT NULL,
        handles_before_ms INTEGER NOT NULL,
        handles_after_ms INTEGER NOT NULL,
        superseded_by TEXT,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (revision) REFERENCES revisions(revision),
        FOREIGN KEY (asset_id) REFERENCES media_asset(id)
    );
    CREATE TABLE export_artifact (
        id TEXT PRIMARY KEY NOT NULL,
        revision INTEGER NOT NULL,
        asset_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        path TEXT NOT NULL,
        sha256 TEXT,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (revision) REFERENCES revisions(revision),
        FOREIGN KEY (asset_id) REFERENCES media_asset(id)
    );
";

const MIGRATIONS: &[(u32, &str)] = &[(1, MIGRATION_V1), (2, MIGRATION_V2)];

pub struct ProjectStore {
    connection: Connection,
}

impl ProjectStore {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
        let current: u32 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        for (version, sql) in MIGRATIONS {
            if *version > current {
                let tx = connection.transaction()?;
                tx.execute_batch(sql)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    params![version],
                )?;
                tx.commit()?;
            }
        }
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::{MIGRATION_V1, ProjectStore};

    fn temp_db_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("double-love-{label}-{unique}.sqlite"))
    }

    #[test]
    fn creates_a_local_project_store_with_foreign_keys_and_wal() {
        let path = temp_db_path("create");

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

    #[test]
    fn migrates_a_v1_store_to_v2_and_is_idempotent() {
        let path = temp_db_path("migrate");

        // 手工落一个只有 v1 的旧库（模拟 PR #45 时代的项目文件）。
        {
            let connection = Connection::open(&path).expect("v1 database opens");
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (
                        version INTEGER PRIMARY KEY NOT NULL,
                        applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                    );",
                )
                .expect("schema_migrations table");
            connection
                .execute_batch(MIGRATION_V1)
                .expect("v1 schema applies");
            connection
                .execute("INSERT INTO schema_migrations(version) VALUES (1)", [])
                .expect("v1 is recorded");
        }

        let store = ProjectStore::open(&path).expect("v1 store migrates");
        drop(store);

        let connection = Connection::open(&path).expect("migrated database opens");
        let versions: Vec<u32> = {
            let mut statement = connection
                .prepare("SELECT version FROM schema_migrations ORDER BY version")
                .expect("versions query");
            statement
                .query_map([], |row| row.get(0))
                .expect("versions read")
                .collect::<Result<Vec<u32>, _>>()
                .expect("versions decode")
        };
        assert_eq!(versions, vec![1, 2]);
        for table in [
            "media_asset",
            "transcript_word",
            "edit_operation",
            "export_artifact",
        ] {
            let exists: bool = connection
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("table existence query");
            assert!(exists, "table {table} should exist after migration");
        }
        // 词序唯一约束来自迁移本身，而不是事后补丁：重复 (asset_id, ordinal) 必须被拒绝。
        connection
            .execute(
                "INSERT INTO media_asset(
                    id, kind, original_path, display_name, duration_samples,
                    audio_sample_rate, fps_num, fps_den, video_timebase, is_ntsc, ffprobe_json
                 ) VALUES ('a1', 'video', '/tmp/synthetic.mp4', 'synthetic', 480000,
                    48000, 25, 1, 25, 0, '{}')",
                [],
            )
            .expect("parent asset inserts");
        connection
            .execute(
                "INSERT INTO transcript_word(
                    word_id, asset_id, ordinal, raw_text, display_text, start_sample, end_sample
                 ) VALUES ('w1', 'a1', 0, '你', '你', 0, 4800)",
                [],
            )
            .expect("first word inserts");
        let duplicate = connection.execute(
            "INSERT INTO transcript_word(
                word_id, asset_id, ordinal, raw_text, display_text, start_sample, end_sample
             ) VALUES ('w2', 'a1', 0, '好', '好', 4800, 9600)",
            [],
        );
        assert!(duplicate.is_err(), "UNIQUE(asset_id, ordinal) must hold");
        drop(connection);

        // 再次打开不得重复迁移、不得报错。
        let store = ProjectStore::open(&path).expect("reopen is idempotent");
        drop(store);

        fs::remove_file(path).expect("temporary database is removed");
    }
}
