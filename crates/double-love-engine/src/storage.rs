use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("expected row missing after write: {0}")]
    MissingRow(String),
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

/// 新导入的媒体资产（写入用）。
pub struct NewMediaAsset {
    pub id: String,
    pub kind: String,
    pub original_path: String,
    pub display_name: String,
    pub duration_samples: i64,
    pub audio_sample_rate: i64,
    pub fps_num: i64,
    pub fps_den: i64,
    pub video_timebase: i64,
    pub is_ntsc: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub audio_channels: Option<i64>,
    pub source_tc_start_frame: Option<i64>,
    pub ffprobe_json: String,
}

/// 媒体资产行（读出用）。
pub struct MediaAssetRow {
    pub id: String,
    pub kind: String,
    pub original_path: String,
    pub display_name: String,
    pub duration_samples: i64,
    pub audio_sample_rate: i64,
    pub fps_num: i64,
    pub fps_den: i64,
    pub video_timebase: i64,
    pub is_ntsc: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub audio_channels: Option<i64>,
    pub source_tc_start_frame: Option<i64>,
    pub prepared_wav_path: Option<String>,
    pub status: String,
}

/// 新转录词（写入用；synthetic 恒 0，source_word_ids 恒 NULL——切片不产合成词）。
pub struct NewTranscriptWord {
    pub word_id: String,
    pub asset_id: String,
    pub ordinal: i64,
    pub raw_text: String,
    pub display_text: String,
    pub language: Option<String>,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
}

const MEDIA_ASSET_COLUMNS: &str = "
    id, kind, original_path, display_name, duration_samples, audio_sample_rate,
    fps_num, fps_den, video_timebase, is_ntsc, width, height, audio_channels,
    source_tc_start_frame, prepared_wav_path, status
";

fn map_media_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaAssetRow> {
    Ok(MediaAssetRow {
        id: row.get("id")?,
        kind: row.get("kind")?,
        original_path: row.get("original_path")?,
        display_name: row.get("display_name")?,
        duration_samples: row.get("duration_samples")?,
        audio_sample_rate: row.get("audio_sample_rate")?,
        fps_num: row.get("fps_num")?,
        fps_den: row.get("fps_den")?,
        video_timebase: row.get("video_timebase")?,
        is_ntsc: row.get("is_ntsc")?,
        width: row.get("width")?,
        height: row.get("height")?,
        audio_channels: row.get("audio_channels")?,
        source_tc_start_frame: row.get("source_tc_start_frame")?,
        prepared_wav_path: row.get("prepared_wav_path")?,
        status: row.get("status")?,
    })
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

    pub fn insert_media_asset(&self, asset: &NewMediaAsset) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO media_asset(
                id, kind, original_path, display_name, duration_samples, audio_sample_rate,
                fps_num, fps_den, video_timebase, is_ntsc, width, height, audio_channels,
                source_tc_start_frame, ffprobe_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                asset.id,
                asset.kind,
                asset.original_path,
                asset.display_name,
                asset.duration_samples,
                asset.audio_sample_rate,
                asset.fps_num,
                asset.fps_den,
                asset.video_timebase,
                asset.is_ntsc,
                asset.width,
                asset.height,
                asset.audio_channels,
                asset.source_tc_start_frame,
                asset.ffprobe_json,
            ],
        )?;
        Ok(())
    }

    pub fn media_asset_by_path(
        &self,
        original_path: &str,
    ) -> Result<Option<MediaAssetRow>, StorageError> {
        self.connection
            .query_row(
                &format!("SELECT {MEDIA_ASSET_COLUMNS} FROM media_asset WHERE original_path = ?1"),
                params![original_path],
                map_media_asset,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn media_asset(&self, id: &str) -> Result<Option<MediaAssetRow>, StorageError> {
        self.connection
            .query_row(
                &format!("SELECT {MEDIA_ASSET_COLUMNS} FROM media_asset WHERE id = ?1"),
                params![id],
                map_media_asset,
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// 全部媒体资产（导入时间升序，同事按 id 稳定）。
    pub fn media_assets(&self) -> Result<Vec<MediaAssetRow>, StorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MEDIA_ASSET_COLUMNS} FROM media_asset ORDER BY imported_at, id"
        ))?;
        let rows = statement
            .query_map([], map_media_asset)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_asset_prepared(&self, id: &str, wav_path: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE media_asset SET prepared_wav_path = ?2, status = 'prepared' WHERE id = ?1",
            params![id, wav_path],
        )?;
        Ok(())
    }

    pub fn set_asset_status(&self, id: &str, status: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE media_asset SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    /// 批量写入转录词（单事务）；调用方保证 ordinal 从 0 连续。
    pub fn insert_transcript_words(&self, words: &[NewTranscriptWord]) -> Result<(), StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        {
            let mut statement = tx.prepare(
                "INSERT INTO transcript_word(
                    word_id, asset_id, ordinal, raw_text, display_text, language,
                    start_sample, end_sample, confidence, synthetic, source_word_ids_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, NULL)",
            )?;
            for word in words {
                statement.execute(params![
                    word.word_id,
                    word.asset_id,
                    word.ordinal,
                    word.raw_text,
                    word.display_text,
                    word.language,
                    word.start_sample,
                    word.end_sample,
                    word.confidence,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 删除某资产全部转录词（重复转录的全量替换语义），返回删除条数。
    pub fn delete_transcript_words(&self, asset_id: &str) -> Result<usize, StorageError> {
        self.connection
            .execute(
                "DELETE FROM transcript_word WHERE asset_id = ?1",
                params![asset_id],
            )
            .map_err(StorageError::from)
    }

    pub fn count_transcript_words(&self, asset_id: &str) -> Result<u64, StorageError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM transcript_word WHERE asset_id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .map_err(StorageError::from)
    }

    /// 按 ordinal 读取某资产全部转录词。
    pub fn transcript_words(
        &self,
        asset_id: &str,
    ) -> Result<Vec<crate::contracts::WordAnchor>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT word_id, asset_id, ordinal, raw_text, display_text, language,
                    start_sample, end_sample, confidence, synthetic, source_word_ids_json
             FROM transcript_word WHERE asset_id = ?1 ORDER BY ordinal",
        )?;
        let rows = statement
            .query_map(params![asset_id], |row| {
                let synthetic: i64 = row.get("synthetic")?;
                let source_json: Option<String> = row.get("source_word_ids_json")?;
                Ok(crate::contracts::WordAnchor {
                    word_id: row.get("word_id")?,
                    asset_id: row.get("asset_id")?,
                    ordinal: row.get("ordinal")?,
                    raw_text: row.get("raw_text")?,
                    display_text: row.get("display_text")?,
                    language: row.get("language")?,
                    start_sample: row.get("start_sample")?,
                    end_sample: row.get("end_sample")?,
                    confidence: row.get("confidence")?,
                    synthetic: synthetic != 0,
                    source_word_ids: source_json.and_then(|text| serde_json::from_str(&text).ok()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 新开一个修订号（revisions 账本 + operation_log 同事务由调用方组合）。
    fn insert_revision_in(
        tx: &rusqlite::Transaction<'_>,
        operation: &str,
    ) -> Result<u64, StorageError> {
        tx.execute(
            "INSERT INTO revisions(operation) VALUES (?1)",
            params![operation],
        )?;
        Ok(tx.last_insert_rowid() as u64)
    }

    fn log_operation_in(
        tx: &rusqlite::Transaction<'_>,
        revision: u64,
        operation: &str,
        payload: &serde_json::Value,
    ) -> Result<(), StorageError> {
        tx.execute(
            "INSERT INTO operation_log(revision, operation, payload_json) VALUES (?1, ?2, ?3)",
            params![revision, operation, payload.to_string()],
        )?;
        Ok(())
    }

    fn map_edit_operation(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::contracts::EditOperation> {
        let edit_type: String = row.get("type")?;
        let behavior: String = row.get("behavior")?;
        Ok(crate::contracts::EditOperation {
            id: row.get("id")?,
            asset_id: row.get("asset_id")?,
            edit_type: crate::contracts::EditType::parse(&edit_type).ok_or(
                rusqlite::Error::InvalidColumnType(
                    0,
                    "type".to_string(),
                    rusqlite::types::Type::Text,
                ),
            )?,
            behavior: crate::contracts::EditBehavior::parse(&behavior).ok_or(
                rusqlite::Error::InvalidColumnType(
                    0,
                    "behavior".to_string(),
                    rusqlite::types::Type::Text,
                ),
            )?,
            start_ordinal: row.get("start_ordinal")?,
            end_ordinal: row.get("end_ordinal")?,
            handles_before_ms: row.get("handles_before_ms")?,
            handles_after_ms: row.get("handles_after_ms")?,
            superseded_by: row.get("superseded_by")?,
            revision: row.get("revision")?,
            created_at: row.get("created_at")?,
        })
    }

    const EDIT_OPERATION_COLUMNS: &'static str = "
        id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
        handles_before_ms, handles_after_ms, superseded_by, created_at
    ";

    pub fn edit_operation(
        &self,
        id: &str,
    ) -> Result<Option<crate::contracts::EditOperation>, StorageError> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {} FROM edit_operation WHERE id = ?1",
                    Self::EDIT_OPERATION_COLUMNS
                ),
                params![id],
                Self::map_edit_operation,
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// 某资产当前活跃的 omit（未被 supersede），按词序升序。
    pub fn active_omit_operations(
        &self,
        asset_id: &str,
    ) -> Result<Vec<crate::contracts::EditOperation>, StorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM edit_operation
             WHERE asset_id = ?1 AND type = 'omit' AND superseded_by IS NULL
             ORDER BY start_ordinal",
            Self::EDIT_OPERATION_COLUMNS
        ))?;
        let rows = statement
            .query_map(params![asset_id], Self::map_edit_operation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// omit：开修订 + 写操作 + 记日志（单事务），返回落库后的操作。
    pub fn apply_omit(
        &self,
        op_id: &str,
        asset_id: &str,
        start_ordinal: i64,
        end_ordinal: i64,
        handles_before_ms: i64,
        handles_after_ms: i64,
    ) -> Result<crate::contracts::EditOperation, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "edit_omit")?;
        tx.execute(
            "INSERT INTO edit_operation(
                id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                handles_before_ms, handles_after_ms, superseded_by
             ) VALUES (?1, ?2, ?3, 'omit', 'ripple_av', ?4, ?5, ?6, ?7, NULL)",
            params![
                op_id,
                revision,
                asset_id,
                start_ordinal,
                end_ordinal,
                handles_before_ms,
                handles_after_ms,
            ],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "edit_omit",
            &serde_json::json!({
                "id": op_id,
                "asset_id": asset_id,
                "start_ordinal": start_ordinal,
                "end_ordinal": end_ordinal,
                "handles_before_ms": handles_before_ms,
                "handles_after_ms": handles_after_ms,
            }),
        )?;
        tx.commit()?;
        self.edit_operation(op_id)?
            .ok_or_else(|| StorageError::MissingRow(op_id.to_string()))
    }

    /// 导出落账：开修订 + 写 export_artifact + 记日志（单事务），返回修订号。
    pub fn apply_export_artifact(
        &self,
        artifact_id: &str,
        asset_id: &str,
        kind: &str,
        path: &str,
        sha256: &str,
    ) -> Result<u64, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "export_roughcut")?;
        tx.execute(
            "INSERT INTO export_artifact(id, revision, asset_id, kind, path, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![artifact_id, revision, asset_id, kind, path, sha256],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "export_roughcut",
            &serde_json::json!({
                "id": artifact_id,
                "asset_id": asset_id,
                "kind": kind,
                "path": path,
                "sha256": sha256,
            }),
        )?;
        tx.commit()?;
        Ok(revision)
    }

    /// restore：supersede 原 omit + 写 restore 操作 + 按需拆分段（单事务）。
    /// `pieces` 为拆分后仍活跃的 omit 区间（含原 handles）。
    #[allow(clippy::too_many_arguments)]
    pub fn apply_restore(
        &self,
        restore_id: &str,
        original: &crate::contracts::EditOperation,
        start_ordinal: i64,
        end_ordinal: i64,
        pieces: &[(i64, i64)],
        piece_ids: &[String],
    ) -> Result<crate::contracts::EditOperation, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "edit_restore")?;
        tx.execute(
            "UPDATE edit_operation SET superseded_by = ?2 WHERE id = ?1",
            params![original.id, restore_id],
        )?;
        tx.execute(
            "INSERT INTO edit_operation(
                id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                handles_before_ms, handles_after_ms, superseded_by
             ) VALUES (?1, ?2, ?3, 'restore', 'ripple_av', ?4, ?5, ?6, ?7, NULL)",
            params![
                restore_id,
                revision,
                original.asset_id,
                start_ordinal,
                end_ordinal,
                original.handles_before_ms,
                original.handles_after_ms,
            ],
        )?;
        for ((piece_start, piece_end), piece_id) in pieces.iter().zip(piece_ids) {
            tx.execute(
                "INSERT INTO edit_operation(
                    id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                    handles_before_ms, handles_after_ms, superseded_by
                 ) VALUES (?1, ?2, ?3, 'omit', 'ripple_av', ?4, ?5, ?6, ?7, NULL)",
                params![
                    piece_id,
                    revision,
                    original.asset_id,
                    piece_start,
                    piece_end,
                    original.handles_before_ms,
                    original.handles_after_ms,
                ],
            )?;
        }
        Self::log_operation_in(
            &tx,
            revision,
            "edit_restore",
            &serde_json::json!({
                "restore_id": restore_id,
                "original_id": original.id,
                "start_ordinal": start_ordinal,
                "end_ordinal": end_ordinal,
                "pieces": pieces,
            }),
        )?;
        tx.commit()?;
        self.edit_operation(restore_id)?
            .ok_or_else(|| StorageError::MissingRow(restore_id.to_string()))
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
