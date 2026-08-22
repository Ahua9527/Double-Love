use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("expected row missing after write: {0}")]
    MissingRow(String),
    #[error("invalid project metadata: {0}")]
    Metadata(String),
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

/// v3：多素材主轨。原始媒体仍保留在 media_asset；主轨只记录引用、源入出点与顺序。
const MIGRATION_V3: &str = "
    CREATE TABLE main_track_clip (
        id TEXT PRIMARY KEY NOT NULL,
        asset_id TEXT NOT NULL,
        source_in_frame INTEGER NOT NULL,
        source_out_frame INTEGER NOT NULL,
        order_index INTEGER NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (asset_id) REFERENCES media_asset(id)
    );
    CREATE INDEX main_track_clip_order_idx ON main_track_clip(order_index, id);
";

/// v4：说话人区间和项目级身份。声纹向量不落库，只保存可审阅的结论。
const MIGRATION_V4: &str = "
    CREATE TABLE speaker_identity (
        id TEXT PRIMARY KEY NOT NULL,
        display_name TEXT NOT NULL,
        aliases_json TEXT NOT NULL DEFAULT '[]',
        color TEXT NOT NULL,
        confirmed INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE speaker_segment (
        id TEXT PRIMARY KEY NOT NULL,
        asset_id TEXT NOT NULL,
        speaker_id TEXT NOT NULL,
        start_sample INTEGER NOT NULL,
        end_sample INTEGER NOT NULL,
        confidence REAL,
        FOREIGN KEY (asset_id) REFERENCES media_asset(id),
        FOREIGN KEY (speaker_id) REFERENCES speaker_identity(id)
    );
    CREATE INDEX speaker_segment_asset_time_idx ON speaker_segment(asset_id, start_sample, end_sample);
    CREATE TABLE speaker_merge_proposal (
        id TEXT PRIMARY KEY NOT NULL,
        left_speaker_id TEXT NOT NULL,
        right_speaker_id TEXT NOT NULL,
        similarity REAL NOT NULL,
        evidence TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (left_speaker_id) REFERENCES speaker_identity(id),
        FOREIGN KEY (right_speaker_id) REFERENCES speaker_identity(id)
    );
";

/// v5：转录结果版本化。新的词先写入非活动 run，只有完整成功才在一次事务中切换。
/// 旧项目的数据会被收口到每个素材的 `legacy:<asset_id>` run，因此旧编辑记录也不会
/// 意外套用到重新转录后的新词序。
const MIGRATION_V5: &str = "
    CREATE TABLE transcript_run (
        id TEXT PRIMARY KEY NOT NULL,
        asset_id TEXT NOT NULL,
        model TEXT NOT NULL,
        language TEXT NOT NULL,
        status TEXT NOT NULL,
        word_count INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        completed_at TEXT,
        FOREIGN KEY (asset_id) REFERENCES media_asset(id)
    );
    ALTER TABLE media_asset ADD COLUMN active_transcript_run_id TEXT;
    ALTER TABLE edit_operation ADD COLUMN transcript_run_id TEXT;
    CREATE TABLE transcript_word_v5 (
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
        run_id TEXT NOT NULL,
        FOREIGN KEY (asset_id) REFERENCES media_asset(id),
        FOREIGN KEY (run_id) REFERENCES transcript_run(id),
        UNIQUE (run_id, ordinal)
    );
    INSERT OR IGNORE INTO transcript_run(id, asset_id, model, language, status, word_count, completed_at)
        SELECT 'legacy:' || asset_id, asset_id, 'legacy', 'auto', 'succeeded', COUNT(*), CURRENT_TIMESTAMP
        FROM transcript_word GROUP BY asset_id;
    INSERT INTO transcript_word_v5(
        word_id, asset_id, ordinal, raw_text, display_text, language, start_sample, end_sample,
        confidence, synthetic, source_word_ids_json, production_range_json,
        speaker_assignments_json, run_id
    )
        SELECT word_id, asset_id, ordinal, raw_text, display_text, language, start_sample, end_sample,
               confidence, synthetic, source_word_ids_json, production_range_json,
               speaker_assignments_json, 'legacy:' || asset_id
        FROM transcript_word;
    DROP TABLE transcript_word;
    ALTER TABLE transcript_word_v5 RENAME TO transcript_word;
    UPDATE edit_operation
        SET transcript_run_id = 'legacy:' || asset_id
        WHERE transcript_run_id IS NULL;
    UPDATE media_asset
        SET active_transcript_run_id = 'legacy:' || id
        WHERE active_transcript_run_id IS NULL
          AND EXISTS(SELECT 1 FROM transcript_word WHERE transcript_word.asset_id = media_asset.id);
    CREATE INDEX transcript_run_asset_created_idx ON transcript_run(asset_id, created_at DESC);
    CREATE INDEX transcript_word_run_ordinal_idx ON transcript_word(run_id, ordinal);
    CREATE INDEX edit_operation_run_active_idx ON edit_operation(transcript_run_id, superseded_by, start_ordinal);
";

/// v6：声纹向量仅保存在项目本地，用于跨素材“候选”而非自动合并。
const MIGRATION_V6: &str = "
    CREATE TABLE speaker_embedding (
        speaker_id TEXT PRIMARY KEY NOT NULL,
        values_json TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (speaker_id) REFERENCES speaker_identity(id)
    );
    CREATE INDEX speaker_merge_proposal_status_idx ON speaker_merge_proposal(status, similarity DESC);
";

/// v7：说话人合并不删除历史 cluster，而是保留本地审计指向。
const MIGRATION_V7: &str = "
    ALTER TABLE speaker_identity ADD COLUMN merged_into TEXT;
    CREATE INDEX speaker_identity_merged_into_idx ON speaker_identity(merged_into);
";

/// v8：可恢复的项目编辑快照。只存本地项目状态，不含媒体字节或声纹向量。
const MIGRATION_V8: &str = "
    CREATE TABLE project_snapshot (
        revision INTEGER PRIMARY KEY NOT NULL,
        snapshot_json TEXT NOT NULL,
        FOREIGN KEY (revision) REFERENCES revisions(revision)
    );
";

/// v9：源时间码必须保留 DF/NDF，不能仅从 29.97 帧率猜测。
const MIGRATION_V9: &str = "
    ALTER TABLE media_asset ADD COLUMN source_tc_is_drop_frame INTEGER NOT NULL DEFAULT 0;
";

/// v10：历史恢复不能把后来生成的说话人重新带回可见项目状态。保留其本地向量，
/// 但将不属于目标快照的身份归档；向量依然不进入快照、日志或导出物。
const MIGRATION_V10: &str = "
    ALTER TABLE speaker_identity ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX speaker_identity_archived_idx ON speaker_identity(archived, merged_into);
";

const MIGRATIONS: &[(u32, &str)] = &[
    (1, MIGRATION_V1),
    (2, MIGRATION_V2),
    (3, MIGRATION_V3),
    (4, MIGRATION_V4),
    (5, MIGRATION_V5),
    (6, MIGRATION_V6),
    (7, MIGRATION_V7),
    (8, MIGRATION_V8),
    (9, MIGRATION_V9),
    (10, MIGRATION_V10),
];

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
    pub source_tc_is_drop_frame: bool,
    pub ffprobe_json: String,
}

/// 媒体资产行（读出用）。
#[derive(Debug, Clone)]
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
    pub source_tc_is_drop_frame: bool,
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

/// 本地转录的一次不可变尝试。只有 `succeeded` 的 run 可以成为资产当前版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptRun {
    pub id: String,
    pub asset_id: String,
    pub model: String,
    pub language: String,
    pub status: String,
    pub word_count: u64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SnapshotSpeakerIdentity {
    identity: crate::contracts::SpeakerIdentity,
    merged_into: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SnapshotWordSpeakerAssignments {
    word_id: String,
    asset_id: String,
    run_id: String,
    assignments: Vec<crate::contracts::SpeakerAssignment>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SnapshotSpeakerAssignmentRun {
    asset_id: String,
    run_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ProjectSpeakerSnapshot {
    identities: Vec<SnapshotSpeakerIdentity>,
    segments: Vec<crate::contracts::SpeakerSegment>,
    #[serde(default)]
    assignment_runs: Vec<SnapshotSpeakerAssignmentRun>,
    word_assignments: Vec<SnapshotWordSpeakerAssignments>,
    merge_proposals: Vec<crate::contracts::SpeakerMergeProposal>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ProjectSnapshot {
    main_track: Vec<crate::contracts::MainTrackClip>,
    canvas: crate::contracts::CanvasSpec,
    subtitle_style: crate::contracts::SubtitleStyle,
    active_omits: Vec<crate::contracts::EditOperation>,
    /// `None` means a legacy snapshot that predates output-rate history. New snapshots use
    /// `Some(None)` for the automatic “follow first clip” mode.
    #[serde(default)]
    output_rate: Option<Option<crate::rational::FrameRate>>,
    /// `None` means this is a legacy v8/v9 snapshot. Restoring it leaves current
    /// speaker metadata intact instead of treating omitted legacy fields as empty.
    #[serde(default)]
    speaker_state: Option<ProjectSpeakerSnapshot>,
}

const MEDIA_ASSET_COLUMNS: &str = "
    id, kind, original_path, display_name, duration_samples, audio_sample_rate,
    fps_num, fps_den, video_timebase, is_ntsc, width, height, audio_channels,
    source_tc_start_frame, source_tc_is_drop_frame, prepared_wav_path, status
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
        source_tc_is_drop_frame: row.get::<_, i64>("source_tc_is_drop_frame")? != 0,
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
        let store = Self { connection };
        store.backfill_current_snapshot()?;
        Ok(store)
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

    fn project_meta_value(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.connection
            .query_row(
                "SELECT value FROM project_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    fn set_project_meta_value_in(
        tx: &rusqlite::Transaction<'_>,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        tx.execute(
            "INSERT INTO project_meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 没有设置时用稳定默认画布，避免每个调用方各自猜测分辨率和 fit。
    pub fn canvas_spec(&self) -> Result<crate::contracts::CanvasSpec, StorageError> {
        let Some(value) = self.project_meta_value("canvas_spec")? else {
            return Ok(crate::contracts::CanvasSpec::default());
        };
        serde_json::from_str(&value).map_err(|error| StorageError::Metadata(error.to_string()))
    }

    pub fn set_canvas_spec(
        &self,
        canvas: &crate::contracts::CanvasSpec,
    ) -> Result<u64, StorageError> {
        let value = serde_json::to_string(canvas)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "canvas_set")?;
        Self::set_project_meta_value_in(&tx, "canvas_spec", &value)?;
        Self::log_operation_in(
            &tx,
            revision,
            "canvas_set",
            &serde_json::json!({ "canvas": canvas }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }

    pub fn output_rate(&self) -> Result<Option<crate::rational::FrameRate>, StorageError> {
        let Some(value) = self.project_meta_value("output_rate")? else {
            return Ok(None);
        };
        serde_json::from_str(&value)
            .map(Some)
            .map_err(|error| StorageError::Metadata(error.to_string()))
    }

    pub fn set_output_rate(&self, rate: crate::rational::FrameRate) -> Result<u64, StorageError> {
        let value = serde_json::to_string(&rate)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "output_rate_set")?;
        Self::set_project_meta_value_in(&tx, "output_rate", &value)?;
        Self::log_operation_in(
            &tx,
            revision,
            "output_rate_set",
            &serde_json::json!({ "rate": rate }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }

    /// 清除显式输出帧率，回到“跟随主轨第一段素材”的项目默认规则。
    pub fn clear_output_rate(&self) -> Result<u64, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "output_rate_clear")?;
        tx.execute("DELETE FROM project_meta WHERE key = 'output_rate'", [])?;
        Self::log_operation_in(
            &tx,
            revision,
            "output_rate_clear",
            &serde_json::json!({ "rate": null }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }

    pub fn subtitle_style(&self) -> Result<crate::contracts::SubtitleStyle, StorageError> {
        let Some(value) = self.project_meta_value("subtitle_style")? else {
            return Ok(crate::contracts::SubtitleStyle::default());
        };
        serde_json::from_str(&value).map_err(|error| StorageError::Metadata(error.to_string()))
    }

    pub fn set_subtitle_style(
        &self,
        style: &crate::contracts::SubtitleStyle,
    ) -> Result<u64, StorageError> {
        let value = serde_json::to_string(style)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "subtitle_style_set")?;
        Self::set_project_meta_value_in(&tx, "subtitle_style", &value)?;
        Self::log_operation_in(
            &tx,
            revision,
            "subtitle_style_set",
            &serde_json::json!({ "style": style }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }

    fn snapshot_from_tx(tx: &rusqlite::Transaction<'_>) -> Result<ProjectSnapshot, StorageError> {
        let main_track = {
            let mut statement = tx.prepare(
                "SELECT id, asset_id, source_in_frame, source_out_frame, order_index
                 FROM main_track_clip ORDER BY order_index, id",
            )?;
            statement
                .query_map([], Self::map_main_track_clip)?
                .collect::<Result<Vec<_>, _>>()?
        };
        let meta_value = |key: &str| -> Result<Option<String>, StorageError> {
            tx.query_row(
                "SELECT value FROM project_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
        };
        let canvas = meta_value("canvas_spec")?
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StorageError::Metadata(error.to_string()))
            })
            .transpose()?
            .unwrap_or_default();
        let subtitle_style = meta_value("subtitle_style")?
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StorageError::Metadata(error.to_string()))
            })
            .transpose()?
            .unwrap_or_default();
        let output_rate = meta_value("output_rate")?
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StorageError::Metadata(error.to_string()))
            })
            .transpose()?;
        let active_omits = {
            let mut statement = tx.prepare(
                "SELECT e.id, e.revision, e.asset_id, e.type, e.behavior, e.start_ordinal,
                        e.end_ordinal, e.handles_before_ms, e.handles_after_ms, e.superseded_by,
                        e.created_at
                 FROM edit_operation e
                 JOIN media_asset a ON a.id = e.asset_id
                 WHERE e.type = 'omit' AND e.superseded_by IS NULL
                   AND e.transcript_run_id = a.active_transcript_run_id
                 ORDER BY e.asset_id, e.start_ordinal",
            )?;
            statement
                .query_map([], Self::map_edit_operation)?
                .collect::<Result<Vec<_>, _>>()?
        };
        let speaker_state = {
            let identities = {
                let mut statement = tx.prepare(
                    "SELECT id, display_name, aliases_json, color, confirmed, merged_into
                     FROM speaker_identity WHERE archived = 0 ORDER BY id",
                )?;
                statement
                    .query_map([], |row| {
                        let aliases_json: String = row.get(2)?;
                        let confirmed: i64 = row.get(4)?;
                        Ok(SnapshotSpeakerIdentity {
                            identity: crate::contracts::SpeakerIdentity {
                                id: row.get(0)?,
                                display_name: row.get(1)?,
                                aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
                                color: row.get(3)?,
                                confirmed: confirmed != 0,
                            },
                            merged_into: row.get(5)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let segments = {
                let mut statement = tx.prepare(
                    "SELECT id, asset_id, speaker_id, start_sample, end_sample, confidence
                     FROM speaker_segment ORDER BY asset_id, start_sample, end_sample, id",
                )?;
                statement
                    .query_map([], Self::map_speaker_segment)?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let assignment_runs = {
                let mut statement = tx.prepare(
                    "SELECT id, active_transcript_run_id FROM media_asset
                     WHERE active_transcript_run_id IS NOT NULL ORDER BY id",
                )?;
                statement
                    .query_map([], |row| {
                        Ok(SnapshotSpeakerAssignmentRun {
                            asset_id: row.get(0)?,
                            run_id: row.get(1)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let word_rows = {
                let mut statement = tx.prepare(
                    "SELECT word_id, asset_id, run_id, speaker_assignments_json
                     FROM transcript_word
                     WHERE speaker_assignments_json IS NOT NULL
                       AND run_id IN (
                           SELECT active_transcript_run_id FROM media_asset
                           WHERE active_transcript_run_id IS NOT NULL
                       )
                     ORDER BY asset_id, run_id, ordinal",
                )?;
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let word_assignments = word_rows
                .into_iter()
                .map(|(word_id, asset_id, run_id, assignments_json)| {
                    Ok(SnapshotWordSpeakerAssignments {
                        word_id,
                        asset_id,
                        run_id,
                        assignments: serde_json::from_str(&assignments_json)
                            .map_err(|error| StorageError::Metadata(error.to_string()))?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            let merge_proposals = {
                let mut statement = tx.prepare(
                    "SELECT id, left_speaker_id, right_speaker_id, similarity, evidence, status
                     FROM speaker_merge_proposal ORDER BY id",
                )?;
                statement
                    .query_map([], Self::map_speaker_merge_proposal)?
                    .collect::<Result<Vec<_>, _>>()?
            };
            Some(ProjectSpeakerSnapshot {
                identities,
                segments,
                assignment_runs,
                word_assignments,
                merge_proposals,
            })
        };
        Ok(ProjectSnapshot {
            main_track,
            canvas,
            subtitle_style,
            active_omits,
            output_rate: Some(output_rate),
            speaker_state,
        })
    }

    fn capture_snapshot_in(
        tx: &rusqlite::Transaction<'_>,
        revision: u64,
    ) -> Result<(), StorageError> {
        let snapshot = Self::snapshot_from_tx(tx)?;
        let snapshot_json = serde_json::to_string(&snapshot)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        tx.execute(
            "INSERT INTO project_snapshot(revision, snapshot_json) VALUES (?1, ?2)
             ON CONFLICT(revision) DO UPDATE SET snapshot_json = excluded.snapshot_json",
            params![revision, snapshot_json],
        )?;
        Ok(())
    }

    fn backfill_current_snapshot(&self) -> Result<(), StorageError> {
        let revision = self.revision()?;
        if revision == 0 {
            return Ok(());
        }
        let snapshot_json: Option<String> = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM project_snapshot WHERE revision = ?1",
                params![revision],
                |row| row.get(0),
            )
            .optional()?;
        let needs_current_format = snapshot_json
            .map(|value| {
                serde_json::from_str::<ProjectSnapshot>(&value)
                    .map(|snapshot| snapshot.speaker_state.is_none())
                    .map_err(|error| StorageError::Metadata(error.to_string()))
            })
            .transpose()?
            .unwrap_or(true);
        if !needs_current_format {
            return Ok(());
        }
        let tx = self.connection.unchecked_transaction()?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(())
    }

    pub fn revision_history(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::contracts::RevisionHistoryEntry>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT r.revision, r.operation, r.committed_at,
                    EXISTS(SELECT 1 FROM project_snapshot s WHERE s.revision = r.revision)
             FROM revisions r ORDER BY r.revision DESC LIMIT ?1",
        )?;
        statement
            .query_map(params![limit as i64], |row| {
                let restorable: i64 = row.get(3)?;
                Ok(crate::contracts::RevisionHistoryEntry {
                    revision: row.get(0)?,
                    operation: row.get(1)?,
                    committed_at: row.get(2)?,
                    restorable: restorable != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// 恢复历史快照中的主轨、画布、输出帧率、字幕样式、说话人展示状态和仍可匹配
    /// 当前转录版本的文字删减。声纹向量不属于快照，始终只留在本地存储层。
    /// 恢复动作自身会写入新的 revision，历史从不被改写。
    pub fn restore_revision(&self, source_revision: u64) -> Result<u64, StorageError> {
        let snapshot_json: String = self
            .connection
            .query_row(
                "SELECT snapshot_json FROM project_snapshot WHERE revision = ?1",
                params![source_revision],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRow(format!("snapshot:{source_revision}")))?;
        let snapshot: ProjectSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let canvas_json = serde_json::to_string(&snapshot.canvas)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let style_json = serde_json::to_string(&snapshot.subtitle_style)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let output_rate_json = snapshot
            .output_rate
            .as_ref()
            .map(|rate| {
                rate.as_ref()
                    .map(|rate| {
                        serde_json::to_string(rate)
                            .map_err(|error| StorageError::Metadata(error.to_string()))
                    })
                    .transpose()
            })
            .transpose()?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "history_restore")?;
        tx.execute("DELETE FROM main_track_clip", [])?;
        let mut clips = tx.prepare(
            "INSERT INTO main_track_clip(id, asset_id, source_in_frame, source_out_frame, order_index)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for clip in &snapshot.main_track {
            clips.execute(params![
                clip.id,
                clip.source_asset_id,
                clip.source_in_frame,
                clip.source_out_frame,
                clip.order_index,
            ])?;
        }
        drop(clips);
        Self::set_project_meta_value_in(&tx, "canvas_spec", &canvas_json)?;
        Self::set_project_meta_value_in(&tx, "subtitle_style", &style_json)?;
        if let Some(output_rate_json) = output_rate_json {
            if let Some(output_rate_json) = output_rate_json.as_deref() {
                Self::set_project_meta_value_in(&tx, "output_rate", output_rate_json)?;
            } else {
                tx.execute("DELETE FROM project_meta WHERE key = 'output_rate'", [])?;
            }
        }

        let restore_marker = format!("history:{revision}");
        tx.execute(
            "UPDATE edit_operation
             SET superseded_by = ?1
             WHERE type = 'omit' AND superseded_by IS NULL
               AND transcript_run_id IN (
                   SELECT active_transcript_run_id FROM media_asset
                   WHERE active_transcript_run_id IS NOT NULL
               )",
            params![restore_marker],
        )?;
        let mut restored_omits = 0_u64;
        for omit in &snapshot.active_omits {
            let active_run: Option<String> = tx
                .query_row(
                    "SELECT active_transcript_run_id FROM media_asset WHERE id = ?1",
                    params![omit.asset_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            let Some(active_run) = active_run else {
                continue;
            };
            let word_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM transcript_word WHERE run_id = ?1",
                params![active_run],
                |row| row.get(0),
            )?;
            if omit.start_ordinal < 0 || omit.end_ordinal >= word_count {
                continue;
            }
            tx.execute(
                "INSERT INTO edit_operation(
                    id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                    handles_before_ms, handles_after_ms, superseded_by, transcript_run_id
                 ) VALUES (?1, ?2, ?3, 'omit', 'ripple_av', ?4, ?5, ?6, ?7, NULL, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    revision,
                    omit.asset_id,
                    omit.start_ordinal,
                    omit.end_ordinal,
                    omit.handles_before_ms,
                    omit.handles_after_ms,
                    active_run,
                ],
            )?;
            restored_omits += 1;
        }
        let (restored_speakers, restored_speaker_segments, restored_word_assignments) =
            if let Some(speaker_state) = snapshot.speaker_state.as_ref() {
                tx.execute("DELETE FROM speaker_segment", [])?;
                tx.execute("DELETE FROM speaker_merge_proposal", [])?;
                tx.execute(
                    "UPDATE speaker_identity SET archived = 1 WHERE archived = 0",
                    [],
                )?;

                let mut identities = tx.prepare(
                    "INSERT INTO speaker_identity(
                        id, display_name, aliases_json, color, confirmed, merged_into, archived
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
                     ON CONFLICT(id) DO UPDATE SET
                        display_name = excluded.display_name,
                        aliases_json = excluded.aliases_json,
                        color = excluded.color,
                        confirmed = excluded.confirmed,
                        merged_into = excluded.merged_into,
                        archived = 0",
                )?;
                for speaker in &speaker_state.identities {
                    let aliases = serde_json::to_string(&speaker.identity.aliases)
                        .map_err(|error| StorageError::Metadata(error.to_string()))?;
                    identities.execute(params![
                        speaker.identity.id,
                        speaker.identity.display_name,
                        aliases,
                        speaker.identity.color,
                        i64::from(speaker.identity.confirmed),
                        speaker.merged_into,
                    ])?;
                }
                drop(identities);

                let mut segments = tx.prepare(
                    "INSERT INTO speaker_segment(
                        id, asset_id, speaker_id, start_sample, end_sample, confidence
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for segment in &speaker_state.segments {
                    segments.execute(params![
                        segment.id,
                        segment.asset_id,
                        segment.speaker_id,
                        segment.start_sample,
                        segment.end_sample,
                        segment.confidence,
                    ])?;
                }
                drop(segments);

                for run in &speaker_state.assignment_runs {
                    tx.execute(
                        "UPDATE transcript_word SET speaker_assignments_json = NULL
                         WHERE asset_id = ?1 AND run_id = ?2
                           AND run_id = (
                               SELECT active_transcript_run_id FROM media_asset WHERE id = ?1
                           )",
                        params![run.asset_id, run.run_id],
                    )?;
                }
                let mut restored_word_assignments = 0_u64;
                for word in &speaker_state.word_assignments {
                    let assignments = serde_json::to_string(&word.assignments)
                        .map_err(|error| StorageError::Metadata(error.to_string()))?;
                    restored_word_assignments += tx.execute(
                        "UPDATE transcript_word SET speaker_assignments_json = ?4
                         WHERE word_id = ?1 AND asset_id = ?2 AND run_id = ?3
                           AND run_id = (
                               SELECT active_transcript_run_id FROM media_asset WHERE id = ?2
                           )",
                        params![word.word_id, word.asset_id, word.run_id, assignments],
                    )? as u64;
                }

                let mut proposals = tx.prepare(
                    "INSERT INTO speaker_merge_proposal(
                        id, left_speaker_id, right_speaker_id, similarity, evidence, status
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for proposal in &speaker_state.merge_proposals {
                    proposals.execute(params![
                        proposal.id,
                        proposal.left_speaker_id,
                        proposal.right_speaker_id,
                        proposal.similarity,
                        proposal.evidence,
                        proposal.status,
                    ])?;
                }
                drop(proposals);
                (
                    Some(speaker_state.identities.len() as u64),
                    Some(speaker_state.segments.len() as u64),
                    Some(restored_word_assignments),
                )
            } else {
                (None, None, None)
            };
        Self::log_operation_in(
            &tx,
            revision,
            "history_restore",
            &serde_json::json!({
                "source_revision": source_revision,
                "restored_main_track_clips": snapshot.main_track.len(),
                "restored_omits": restored_omits,
                "restored_speakers": restored_speakers,
                "restored_speaker_segments": restored_speaker_segments,
                "restored_word_assignments": restored_word_assignments,
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
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
                source_tc_start_frame, source_tc_is_drop_frame, ffprobe_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                asset.source_tc_is_drop_frame,
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

    fn map_transcript_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptRun> {
        Ok(TranscriptRun {
            id: row.get("id")?,
            asset_id: row.get("asset_id")?,
            model: row.get("model")?,
            language: row.get("language")?,
            status: row.get("status")?,
            word_count: row.get("word_count")?,
            created_at: row.get("created_at")?,
            completed_at: row.get("completed_at")?,
        })
    }

    /// 当前展示和剪辑所使用的转录版本。没有成功转录时为 None。
    pub fn active_transcript_run_id(&self, asset_id: &str) -> Result<Option<String>, StorageError> {
        self.connection
            .query_row(
                "SELECT active_transcript_run_id FROM media_asset WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
            .map(|value: Option<Option<String>>| value.flatten())
    }

    pub fn transcript_runs(&self, asset_id: &str) -> Result<Vec<TranscriptRun>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, asset_id, model, language, status, word_count, created_at, completed_at
             FROM transcript_run WHERE asset_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        statement
            .query_map(params![asset_id], Self::map_transcript_run)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// 建立一个不可见的候选转录版本。该步骤不改变正在编辑的活动版本。
    pub fn begin_transcript_run(
        &self,
        asset_id: &str,
        model: &str,
        language: &str,
    ) -> Result<TranscriptRun, StorageError> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM media_asset WHERE id = ?1)",
            params![asset_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::MissingRow(asset_id.to_string()));
        }
        let id = Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO transcript_run(id, asset_id, model, language, status)
             VALUES (?1, ?2, ?3, ?4, 'running')",
            params![id, asset_id, model, language],
        )?;
        self.connection
            .query_row(
                "SELECT id, asset_id, model, language, status, word_count, created_at, completed_at
                 FROM transcript_run WHERE id = ?1",
                params![id],
                Self::map_transcript_run,
            )
            .map_err(StorageError::from)
    }

    /// 标记一次不可见候选的终态。失败、取消、部分结果均不切换当前版本。
    pub fn finish_transcript_run(&self, run_id: &str, status: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE transcript_run SET status = ?2, completed_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![run_id, status],
        )?;
        Ok(())
    }

    /// 将候选 run 原子切为活动版本，并把新版本的词数记入本地修订账本。
    pub fn activate_transcript_run(&self, run_id: &str) -> Result<u64, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let asset_id: String = tx
            .query_row(
                "SELECT asset_id FROM transcript_run WHERE id = ?1 AND status = 'running'",
                params![run_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRow(run_id.to_string()))?;
        let word_count: u64 = tx.query_row(
            "SELECT COUNT(*) FROM transcript_word WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;
        let revision = Self::insert_revision_in(&tx, "transcript_activate")?;
        tx.execute(
            "UPDATE transcript_run
             SET status = 'succeeded', word_count = ?2, completed_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![run_id, word_count],
        )?;
        tx.execute(
            "UPDATE media_asset
             SET active_transcript_run_id = ?2, status = 'transcribed'
             WHERE id = ?1",
            params![asset_id, run_id],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "transcript_activate",
            &serde_json::json!({
                "asset_id": asset_id,
                "run_id": run_id,
                "word_count": word_count,
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }

    fn direct_transcript_run(&self, asset_id: &str) -> Result<String, StorageError> {
        if let Some(run_id) = self.active_transcript_run_id(asset_id)? {
            return Ok(run_id);
        }
        let run_id = format!("manual:{}", Uuid::new_v4());
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO transcript_run(id, asset_id, model, language, status, completed_at)
             VALUES (?1, ?2, 'manual', 'auto', 'succeeded', CURRENT_TIMESTAMP)",
            params![run_id, asset_id],
        )?;
        tx.execute(
            "UPDATE media_asset SET active_transcript_run_id = ?2, status = 'transcribed' WHERE id = ?1",
            params![asset_id, run_id],
        )?;
        tx.commit()?;
        Ok(run_id)
    }

    /// 批量写入转录词（单事务）；调用方保证 ordinal 从 0 连续。
    /// 该兼容入口用于测试和已有调用者，会写入当前活动版本；转录任务必须使用
    /// `insert_transcript_words_for_run`，以免候选词提前出现在界面中。
    pub fn insert_transcript_words(&self, words: &[NewTranscriptWord]) -> Result<(), StorageError> {
        let Some(first) = words.first() else {
            return Ok(());
        };
        if words.iter().any(|word| word.asset_id != first.asset_id) {
            return Err(StorageError::Metadata(
                "一次写入只能属于同一个素材的转录版本".to_string(),
            ));
        }
        let run_id = self.direct_transcript_run(&first.asset_id)?;
        self.insert_transcript_words_for_run(&run_id, words)
    }

    /// 向不可见或活动的指定版本写入词。所有词必须归属该 run 的同一素材。
    pub fn insert_transcript_words_for_run(
        &self,
        run_id: &str,
        words: &[NewTranscriptWord],
    ) -> Result<(), StorageError> {
        let Some(first) = words.first() else {
            return Ok(());
        };
        if words.iter().any(|word| word.asset_id != first.asset_id) {
            return Err(StorageError::Metadata(
                "一次写入只能属于同一个素材的转录版本".to_string(),
            ));
        }
        let tx = self.connection.unchecked_transaction()?;
        let run_asset_id: String = tx
            .query_row(
                "SELECT asset_id FROM transcript_run WHERE id = ?1 AND status IN ('running', 'succeeded')",
                params![run_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRow(run_id.to_string()))?;
        if run_asset_id != first.asset_id {
            return Err(StorageError::Metadata(
                "转录版本与写入素材不匹配".to_string(),
            ));
        }
        {
            let mut statement = tx.prepare(
                "INSERT INTO transcript_word(
                    word_id, asset_id, ordinal, raw_text, display_text, language,
                    start_sample, end_sample, confidence, synthetic, source_word_ids_json, run_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, NULL, ?10)",
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
                    run_id,
                ])?;
            }
        }
        tx.execute(
            "UPDATE transcript_run
             SET word_count = (SELECT COUNT(*) FROM transcript_word WHERE run_id = ?1)
             WHERE id = ?1",
            params![run_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 删除当前活动版本的词。新转录不再调用此入口，而是在成功后原子切换版本。
    pub fn delete_transcript_words(&self, asset_id: &str) -> Result<usize, StorageError> {
        let Some(run_id) = self.active_transcript_run_id(asset_id)? else {
            return Ok(0);
        };
        self.connection
            .execute(
                "DELETE FROM transcript_word WHERE asset_id = ?1 AND run_id = ?2",
                params![asset_id, run_id],
            )
            .map_err(StorageError::from)
    }

    pub fn count_transcript_words(&self, asset_id: &str) -> Result<u64, StorageError> {
        let Some(run_id) = self.active_transcript_run_id(asset_id)? else {
            return Ok(0);
        };
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM transcript_word WHERE asset_id = ?1 AND run_id = ?2",
                params![asset_id, run_id],
                |row| row.get(0),
            )
            .map_err(StorageError::from)
    }

    /// 按 ordinal 读取某资产全部转录词。
    pub fn transcript_words(
        &self,
        asset_id: &str,
    ) -> Result<Vec<crate::contracts::WordAnchor>, StorageError> {
        let Some(run_id) = self.active_transcript_run_id(asset_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = self.connection.prepare(
            "SELECT word_id, asset_id, ordinal, raw_text, display_text, language,
                    start_sample, end_sample, confidence, synthetic, source_word_ids_json,
                    speaker_assignments_json
             FROM transcript_word WHERE asset_id = ?1 AND run_id = ?2 ORDER BY ordinal",
        )?;
        let rows = statement
            .query_map(params![asset_id, run_id], |row| {
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
                    speaker_assignments: row
                        .get::<_, Option<String>>("speaker_assignments_json")?
                        .and_then(|text| serde_json::from_str(&text).ok())
                        .unwrap_or_default(),
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
        let Some(run_id) = self.active_transcript_run_id(asset_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM edit_operation
             WHERE asset_id = ?1 AND transcript_run_id = ?2
               AND type = 'omit' AND superseded_by IS NULL
             ORDER BY start_ordinal",
            Self::EDIT_OPERATION_COLUMNS
        ))?;
        let rows = statement
            .query_map(params![asset_id, run_id], Self::map_edit_operation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 编辑操作所属于的转录版本。用于拒绝对历史版本进行恢复，防止词序错配。
    pub fn edit_operation_is_on_active_run(&self, id: &str) -> Result<bool, StorageError> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM edit_operation
                    JOIN media_asset ON media_asset.id = edit_operation.asset_id
                    WHERE edit_operation.id = ?1
                      AND edit_operation.transcript_run_id = media_asset.active_transcript_run_id
                )",
                params![id],
                |row| row.get(0),
            )
            .map_err(StorageError::from)
    }

    fn edit_operation_run_id(&self, id: &str) -> Result<String, StorageError> {
        self.connection
            .query_row(
                "SELECT transcript_run_id FROM edit_operation WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(StorageError::from)?
            .flatten()
            .ok_or_else(|| StorageError::MissingRow(id.to_string()))
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
        let run_id = self
            .active_transcript_run_id(asset_id)?
            .ok_or_else(|| StorageError::MissingRow(format!("active transcript for {asset_id}")))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "edit_omit")?;
        tx.execute(
            "INSERT INTO edit_operation(
                id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                handles_before_ms, handles_after_ms, superseded_by, transcript_run_id
             ) VALUES (?1, ?2, ?3, 'omit', 'ripple_av', ?4, ?5, ?6, ?7, NULL, ?8)",
            params![
                op_id,
                revision,
                asset_id,
                start_ordinal,
                end_ordinal,
                handles_before_ms,
                handles_after_ms,
                run_id,
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
                "transcript_run_id": run_id,
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
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
        let operation = match kind {
            "ass" => "export_ass",
            "mp4_burned_subtitles" => "export_mp4",
            "premiere_resolve_xmeml" => "export_xmeml",
            _ => "export_roughcut",
        };
        let revision = Self::insert_revision_in(&tx, operation)?;
        tx.execute(
            "INSERT INTO export_artifact(id, revision, asset_id, kind, path, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![artifact_id, revision, asset_id, kind, path, sha256],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            operation,
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
        let run_id = self.edit_operation_run_id(&original.id)?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "edit_restore")?;
        tx.execute(
            "UPDATE edit_operation SET superseded_by = ?2 WHERE id = ?1",
            params![original.id, restore_id],
        )?;
        tx.execute(
            "INSERT INTO edit_operation(
                id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                handles_before_ms, handles_after_ms, superseded_by, transcript_run_id
             ) VALUES (?1, ?2, ?3, 'restore', 'ripple_av', ?4, ?5, ?6, ?7, NULL, ?8)",
            params![
                restore_id,
                revision,
                original.asset_id,
                start_ordinal,
                end_ordinal,
                original.handles_before_ms,
                original.handles_after_ms,
                run_id,
            ],
        )?;
        for ((piece_start, piece_end), piece_id) in pieces.iter().zip(piece_ids) {
            tx.execute(
                "INSERT INTO edit_operation(
                    id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                    handles_before_ms, handles_after_ms, superseded_by, transcript_run_id
                 ) VALUES (?1, ?2, ?3, 'omit', 'ripple_av', ?4, ?5, ?6, ?7, NULL, ?8)",
                params![
                    piece_id,
                    revision,
                    original.asset_id,
                    piece_start,
                    piece_end,
                    original.handles_before_ms,
                    original.handles_after_ms,
                    run_id,
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
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        self.edit_operation(restore_id)?
            .ok_or_else(|| StorageError::MissingRow(restore_id.to_string()))
    }

    fn map_main_track_clip(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::contracts::MainTrackClip> {
        Ok(crate::contracts::MainTrackClip {
            id: row.get("id")?,
            source_asset_id: row.get("asset_id")?,
            source_in_frame: row.get("source_in_frame")?,
            source_out_frame: row.get("source_out_frame")?,
            order_index: row.get("order_index")?,
        })
    }

    const MAIN_TRACK_COLUMNS: &'static str =
        "id, asset_id, source_in_frame, source_out_frame, order_index";

    pub fn main_track_clips(&self) -> Result<Vec<crate::contracts::MainTrackClip>, StorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM main_track_clip ORDER BY order_index, id",
            Self::MAIN_TRACK_COLUMNS
        ))?;
        statement
            .query_map([], Self::map_main_track_clip)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn main_track_clip(
        &self,
        id: &str,
    ) -> Result<Option<crate::contracts::MainTrackClip>, StorageError> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {} FROM main_track_clip WHERE id = ?1",
                    Self::MAIN_TRACK_COLUMNS
                ),
                params![id],
                Self::map_main_track_clip,
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// 将源素材的一段追加到主轨末尾；所有顺序写入与 revision 同事务提交。
    pub fn append_main_track_clip(
        &self,
        id: &str,
        asset_id: &str,
        source_in_frame: i64,
        source_out_frame: i64,
    ) -> Result<crate::contracts::MainTrackClip, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let order_index: i64 = tx.query_row(
            "SELECT COALESCE(MAX(order_index), -1) + 1 FROM main_track_clip",
            [],
            |row| row.get(0),
        )?;
        let revision = Self::insert_revision_in(&tx, "main_track_append")?;
        tx.execute(
            "INSERT INTO main_track_clip(id, asset_id, source_in_frame, source_out_frame, order_index)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, asset_id, source_in_frame, source_out_frame, order_index],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "main_track_append",
            &serde_json::json!({
                "id": id,
                "asset_id": asset_id,
                "source_in_frame": source_in_frame,
                "source_out_frame": source_out_frame,
                "order_index": order_index,
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        self.main_track_clip(id)?
            .ok_or_else(|| StorageError::MissingRow(id.to_string()))
    }

    /// 将片段移动到某片段之前；None 代表主轨末尾。
    pub fn move_main_track_clip(
        &self,
        id: &str,
        before_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut clips = self.main_track_clips()?;
        let Some(from) = clips.iter().position(|clip| clip.id == id) else {
            return Err(StorageError::MissingRow(id.to_string()));
        };
        let clip = clips.remove(from);
        let target = match before_id {
            Some(before) => clips
                .iter()
                .position(|candidate| candidate.id == before)
                .ok_or_else(|| StorageError::MissingRow(before.to_string()))?,
            None => clips.len(),
        };
        clips.insert(target, clip);

        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "main_track_move")?;
        for (index, clip) in clips.iter().enumerate() {
            tx.execute(
                "UPDATE main_track_clip SET order_index = ?2 WHERE id = ?1",
                params![clip.id, index as i64],
            )?;
        }
        Self::log_operation_in(
            &tx,
            revision,
            "main_track_move",
            &serde_json::json!({ "id": id, "before_id": before_id }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(())
    }

    pub fn trim_main_track_clip(
        &self,
        id: &str,
        source_in_frame: i64,
        source_out_frame: i64,
    ) -> Result<crate::contracts::MainTrackClip, StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "main_track_trim")?;
        let changed = tx.execute(
            "UPDATE main_track_clip
             SET source_in_frame = ?2, source_out_frame = ?3
             WHERE id = ?1",
            params![id, source_in_frame, source_out_frame],
        )?;
        if changed == 0 {
            return Err(StorageError::MissingRow(id.to_string()));
        }
        Self::log_operation_in(
            &tx,
            revision,
            "main_track_trim",
            &serde_json::json!({
                "id": id,
                "source_in_frame": source_in_frame,
                "source_out_frame": source_out_frame,
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        self.main_track_clip(id)?
            .ok_or_else(|| StorageError::MissingRow(id.to_string()))
    }

    /// 将一条主轨片段在源帧边界拆为两条，后半段插在原片段之后。
    pub fn split_main_track_clip(
        &self,
        id: &str,
        right_id: &str,
        source_at_frame: i64,
    ) -> Result<
        (
            crate::contracts::MainTrackClip,
            crate::contracts::MainTrackClip,
        ),
        StorageError,
    > {
        let original = self
            .main_track_clip(id)?
            .ok_or_else(|| StorageError::MissingRow(id.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "main_track_split")?;
        tx.execute(
            "UPDATE main_track_clip SET order_index = order_index + 1 WHERE order_index > ?1",
            params![original.order_index],
        )?;
        tx.execute(
            "UPDATE main_track_clip SET source_out_frame = ?2 WHERE id = ?1",
            params![id, source_at_frame],
        )?;
        tx.execute(
            "INSERT INTO main_track_clip(id, asset_id, source_in_frame, source_out_frame, order_index)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                right_id,
                original.source_asset_id,
                source_at_frame,
                original.source_out_frame,
                original.order_index + 1,
            ],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "main_track_split",
            &serde_json::json!({ "id": id, "right_id": right_id, "at": source_at_frame }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        let left = self
            .main_track_clip(id)?
            .ok_or_else(|| StorageError::MissingRow(id.to_string()))?;
        let right = self
            .main_track_clip(right_id)?
            .ok_or_else(|| StorageError::MissingRow(right_id.to_string()))?;
        Ok((left, right))
    }

    pub fn remove_main_track_clip(&self, id: &str) -> Result<(), StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "main_track_remove")?;
        let changed = tx.execute("DELETE FROM main_track_clip WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(StorageError::MissingRow(id.to_string()));
        }
        let mut statement =
            tx.prepare("SELECT id FROM main_track_clip ORDER BY order_index, id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (index, remaining_id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE main_track_clip SET order_index = ?2 WHERE id = ?1",
                params![remaining_id, index as i64],
            )?;
        }
        Self::log_operation_in(
            &tx,
            revision,
            "main_track_remove",
            &serde_json::json!({ "id": id }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(())
    }

    const SPEAKER_IDENTITY_COLUMNS: &'static str =
        "id, display_name, aliases_json, color, confirmed";

    fn map_speaker_identity(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::contracts::SpeakerIdentity> {
        let aliases_json: String = row.get("aliases_json")?;
        let confirmed: i64 = row.get("confirmed")?;
        Ok(crate::contracts::SpeakerIdentity {
            id: row.get("id")?,
            display_name: row.get("display_name")?,
            aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
            color: row.get("color")?,
            confirmed: confirmed != 0,
        })
    }

    pub fn speaker_identities(
        &self,
    ) -> Result<Vec<crate::contracts::SpeakerIdentity>, StorageError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM speaker_identity WHERE archived = 0 AND merged_into IS NULL
             ORDER BY confirmed DESC, display_name, id",
            Self::SPEAKER_IDENTITY_COLUMNS
        ))?;
        statement
            .query_map([], Self::map_speaker_identity)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn upsert_speaker_identity(
        &self,
        identity: &crate::contracts::SpeakerIdentity,
    ) -> Result<(), StorageError> {
        let aliases = serde_json::to_string(&identity.aliases)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        self.connection.execute(
            "INSERT INTO speaker_identity(id, display_name, aliases_json, color, confirmed, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                aliases_json = excluded.aliases_json,
                color = excluded.color,
                confirmed = excluded.confirmed,
                merged_into = NULL,
                archived = 0",
            params![
                identity.id,
                identity.display_name,
                aliases,
                identity.color,
                identity.confirmed,
            ],
        )?;
        Ok(())
    }

    /// 用户确认姓名后才改展示层；原显示名保留为别名，文字和时间都不被改写。
    pub fn confirm_speaker_name(
        &self,
        speaker_id: &str,
        display_name: &str,
    ) -> Result<crate::contracts::SpeakerIdentity, StorageError> {
        let display_name = display_name.trim();
        if display_name.is_empty() || display_name.chars().count() > 64 {
            return Err(StorageError::Metadata(
                "说话人名称必须为 1–64 个字符".to_string(),
            ));
        }
        let mut identity = self
            .speaker_identities()?
            .into_iter()
            .find(|identity| identity.id == speaker_id)
            .ok_or_else(|| StorageError::MissingRow(speaker_id.to_string()))?;
        if identity.display_name != display_name
            && !identity.aliases.contains(&identity.display_name)
        {
            identity.aliases.push(identity.display_name.clone());
        }
        identity.aliases.retain(|alias| alias != display_name);
        identity.display_name = display_name.to_string();
        identity.confirmed = true;
        let aliases = serde_json::to_string(&identity.aliases)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "speaker_name_confirm")?;
        tx.execute(
            "UPDATE speaker_identity
             SET display_name = ?2, aliases_json = ?3, confirmed = 1
             WHERE id = ?1 AND merged_into IS NULL",
            params![speaker_id, identity.display_name, aliases],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "speaker_name_confirm",
            &serde_json::json!({ "speaker_id": speaker_id, "display_name": display_name }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(identity)
    }

    /// 合并两个项目级匿名 cluster。它只会在用户确认后执行，并把旧 cluster 保留为
    /// `merged_into` 审计记录；不输出也不上传声纹。
    pub fn merge_speaker_identities(
        &self,
        keep_speaker_id: &str,
        merge_speaker_id: &str,
    ) -> Result<crate::contracts::SpeakerIdentity, StorageError> {
        if keep_speaker_id == merge_speaker_id {
            return Err(StorageError::Metadata("不能合并同一个说话人。".to_string()));
        }
        let identities = self.speaker_identities()?;
        let mut keep = identities
            .iter()
            .find(|identity| identity.id == keep_speaker_id)
            .cloned()
            .ok_or_else(|| StorageError::MissingRow(keep_speaker_id.to_string()))?;
        let merged = identities
            .iter()
            .find(|identity| identity.id == merge_speaker_id)
            .cloned()
            .ok_or_else(|| StorageError::MissingRow(merge_speaker_id.to_string()))?;
        if !keep.aliases.contains(&merged.display_name) && keep.display_name != merged.display_name
        {
            keep.aliases.push(merged.display_name.clone());
        }
        for alias in &merged.aliases {
            if alias != &keep.display_name && !keep.aliases.contains(alias) {
                keep.aliases.push(alias.clone());
            }
        }
        let aliases = serde_json::to_string(&keep.aliases)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        let tx = self.connection.unchecked_transaction()?;
        let revision = Self::insert_revision_in(&tx, "speaker_merge_confirm")?;
        tx.execute(
            "UPDATE speaker_segment SET speaker_id = ?1 WHERE speaker_id = ?2",
            params![keep_speaker_id, merge_speaker_id],
        )?;
        let assignment_rows = {
            let mut statement = tx.prepare(
                "SELECT word_id, speaker_assignments_json FROM transcript_word
                 WHERE speaker_assignments_json IS NOT NULL
                   AND run_id IN (
                       SELECT active_transcript_run_id FROM media_asset
                       WHERE active_transcript_run_id IS NOT NULL
                   )",
            )?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for (word_id, json) in assignment_rows {
            let mut assignments: Vec<crate::contracts::SpeakerAssignment> =
                serde_json::from_str(&json)
                    .map_err(|error| StorageError::Metadata(error.to_string()))?;
            let mut changed = false;
            for assignment in &mut assignments {
                if assignment.speaker_id == merge_speaker_id {
                    assignment.speaker_id = keep_speaker_id.to_string();
                    assignment.evidence = "confirmed_speaker_merge".to_string();
                    changed = true;
                }
            }
            if changed {
                tx.execute(
                    "UPDATE transcript_word SET speaker_assignments_json = ?2 WHERE word_id = ?1",
                    params![
                        word_id,
                        serde_json::to_string(&assignments)
                            .map_err(|error| StorageError::Metadata(error.to_string()))?
                    ],
                )?;
            }
        }
        tx.execute(
            "UPDATE speaker_identity SET aliases_json = ?2 WHERE id = ?1",
            params![keep_speaker_id, aliases],
        )?;
        tx.execute(
            "UPDATE speaker_identity SET merged_into = ?2 WHERE id = ?1",
            params![merge_speaker_id, keep_speaker_id],
        )?;
        tx.execute(
            "DELETE FROM speaker_embedding WHERE speaker_id = ?1",
            params![merge_speaker_id],
        )?;
        tx.execute(
            "UPDATE speaker_merge_proposal
             SET status = CASE
                 WHEN (left_speaker_id = ?1 AND right_speaker_id = ?2)
                   OR (left_speaker_id = ?2 AND right_speaker_id = ?1) THEN 'accepted'
                 WHEN left_speaker_id = ?2 OR right_speaker_id = ?2 THEN 'rejected'
                 ELSE status
             END",
            params![keep_speaker_id, merge_speaker_id],
        )?;
        Self::log_operation_in(
            &tx,
            revision,
            "speaker_merge_confirm",
            &serde_json::json!({ "keep_speaker_id": keep_speaker_id, "merge_speaker_id": merge_speaker_id }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(keep)
    }

    /// 声纹向量是敏感本地数据：只存 SQLite，不进入 operation_log、TS 契约或导出物。
    pub fn upsert_speaker_embedding(
        &self,
        speaker_id: &str,
        values: &[f32],
    ) -> Result<(), StorageError> {
        let values_json = serde_json::to_string(values)
            .map_err(|error| StorageError::Metadata(error.to_string()))?;
        self.connection.execute(
            "INSERT INTO speaker_embedding(speaker_id, values_json)
             VALUES (?1, ?2)
             ON CONFLICT(speaker_id) DO UPDATE SET
                values_json = excluded.values_json,
                updated_at = CURRENT_TIMESTAMP",
            params![speaker_id, values_json],
        )?;
        Ok(())
    }

    pub fn speaker_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT e.speaker_id, e.values_json
                 FROM speaker_embedding e
                 JOIN speaker_identity i ON i.id = e.speaker_id
                 WHERE i.archived = 0 AND i.merged_into IS NULL
                 ORDER BY e.speaker_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                let values: String = row.get("values_json")?;
                let values = serde_json::from_str(&values).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok((row.get("speaker_id")?, values))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn map_speaker_merge_proposal(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::contracts::SpeakerMergeProposal> {
        Ok(crate::contracts::SpeakerMergeProposal {
            id: row.get("id")?,
            left_speaker_id: row.get("left_speaker_id")?,
            right_speaker_id: row.get("right_speaker_id")?,
            similarity: row.get("similarity")?,
            evidence: row.get("evidence")?,
            status: row.get("status")?,
        })
    }

    pub fn speaker_merge_proposals(
        &self,
    ) -> Result<Vec<crate::contracts::SpeakerMergeProposal>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, left_speaker_id, right_speaker_id, similarity, evidence, status
             FROM speaker_merge_proposal ORDER BY status = 'pending' DESC, similarity DESC, id",
        )?;
        statement
            .query_map([], Self::map_speaker_merge_proposal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// 仅替换尚未处理的候选；用户已接受/拒绝的记录保留在项目历史中。
    pub fn replace_pending_speaker_merge_proposals(
        &self,
        proposals: &[crate::contracts::SpeakerMergeProposal],
    ) -> Result<(), StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM speaker_merge_proposal WHERE status = 'pending'",
            [],
        )?;
        let mut statement = tx.prepare(
            "INSERT INTO speaker_merge_proposal(
                id, left_speaker_id, right_speaker_id, similarity, evidence, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
        )?;
        for proposal in proposals {
            statement.execute(params![
                proposal.id,
                proposal.left_speaker_id,
                proposal.right_speaker_id,
                proposal.similarity,
                proposal.evidence,
            ])?;
        }
        drop(statement);
        tx.commit()?;
        Ok(())
    }

    fn map_speaker_segment(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::contracts::SpeakerSegment> {
        Ok(crate::contracts::SpeakerSegment {
            id: row.get("id")?,
            asset_id: row.get("asset_id")?,
            speaker_id: row.get("speaker_id")?,
            start_sample: row.get("start_sample")?,
            end_sample: row.get("end_sample")?,
            confidence: row.get("confidence")?,
        })
    }

    pub fn speaker_segments(
        &self,
        asset_id: &str,
    ) -> Result<Vec<crate::contracts::SpeakerSegment>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id, asset_id, speaker_id, start_sample, end_sample, confidence
             FROM speaker_segment WHERE asset_id = ?1 ORDER BY start_sample, end_sample, id",
        )?;
        statement
            .query_map(params![asset_id], Self::map_speaker_segment)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn replace_speaker_segments(
        &self,
        asset_id: &str,
        segments: &[crate::contracts::SpeakerSegment],
    ) -> Result<(), StorageError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM speaker_segment WHERE asset_id = ?1",
            params![asset_id],
        )?;
        let mut statement = tx.prepare(
            "INSERT INTO speaker_segment(id, asset_id, speaker_id, start_sample, end_sample, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for segment in segments {
            statement.execute(params![
                segment.id,
                segment.asset_id,
                segment.speaker_id,
                segment.start_sample,
                segment.end_sample,
                segment.confidence,
            ])?;
        }
        drop(statement);
        tx.commit()?;
        Ok(())
    }

    pub fn set_word_speaker_assignments(
        &self,
        asset_id: &str,
        assignments: &[(String, Vec<crate::contracts::SpeakerAssignment>)],
    ) -> Result<(), StorageError> {
        let Some(run_id) = self.active_transcript_run_id(asset_id)? else {
            return Ok(());
        };
        let tx = self.connection.unchecked_transaction()?;
        let mut statement = tx.prepare(
            "UPDATE transcript_word SET speaker_assignments_json = ?3
             WHERE asset_id = ?1 AND word_id = ?2 AND run_id = ?4",
        )?;
        for (word_id, assignment) in assignments {
            let json = serde_json::to_string(assignment)
                .map_err(|error| StorageError::Metadata(error.to_string()))?;
            statement.execute(params![asset_id, word_id, json, run_id])?;
        }
        drop(statement);
        tx.commit()?;
        Ok(())
    }

    /// 将一次已完整返回的本地说话人分析原子写入项目。嵌入只进入本地表；修订日志和
    /// 快照仅记录可审阅的身份、区间和词归属，绝不包含 embedding 数值。
    #[allow(clippy::too_many_arguments)]
    pub fn apply_speaker_diarization(
        &self,
        asset_id: &str,
        identities: &[crate::contracts::SpeakerIdentity],
        segments: &[crate::contracts::SpeakerSegment],
        assignments: &[(String, Vec<crate::contracts::SpeakerAssignment>)],
        embedding_updates: &[(String, Vec<f32>)],
        proposals: &[crate::contracts::SpeakerMergeProposal],
    ) -> Result<u64, StorageError> {
        if segments.iter().any(|segment| segment.asset_id != asset_id) {
            return Err(StorageError::Metadata(
                "说话人区间包含了错误的素材 ID".to_string(),
            ));
        }
        let tx = self.connection.unchecked_transaction()?;
        let active_run: Option<String> = tx
            .query_row(
                "SELECT active_transcript_run_id FROM media_asset WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(active_run) = active_run else {
            return Err(StorageError::Metadata(
                "请先完成转录，再应用说话人分离结果。".to_string(),
            ));
        };
        let revision = Self::insert_revision_in(&tx, "speaker_diarize")?;

        let mut identity_statement = tx.prepare(
            "INSERT INTO speaker_identity(id, display_name, aliases_json, color, confirmed, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                aliases_json = excluded.aliases_json,
                color = excluded.color,
                confirmed = excluded.confirmed,
                merged_into = NULL,
                archived = 0",
        )?;
        for identity in identities {
            let aliases = serde_json::to_string(&identity.aliases)
                .map_err(|error| StorageError::Metadata(error.to_string()))?;
            identity_statement.execute(params![
                identity.id,
                identity.display_name,
                aliases,
                identity.color,
                i64::from(identity.confirmed),
            ])?;
        }
        drop(identity_statement);

        tx.execute(
            "DELETE FROM speaker_segment WHERE asset_id = ?1",
            params![asset_id],
        )?;
        let mut segment_statement = tx.prepare(
            "INSERT INTO speaker_segment(id, asset_id, speaker_id, start_sample, end_sample, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for segment in segments {
            segment_statement.execute(params![
                segment.id,
                segment.asset_id,
                segment.speaker_id,
                segment.start_sample,
                segment.end_sample,
                segment.confidence,
            ])?;
        }
        drop(segment_statement);

        let mut assignment_statement = tx.prepare(
            "UPDATE transcript_word SET speaker_assignments_json = ?3
             WHERE asset_id = ?1 AND word_id = ?2 AND run_id = ?4",
        )?;
        for (word_id, assignment) in assignments {
            let json = serde_json::to_string(assignment)
                .map_err(|error| StorageError::Metadata(error.to_string()))?;
            assignment_statement.execute(params![asset_id, word_id, json, &active_run])?;
        }
        drop(assignment_statement);

        let mut embedding_statement = tx.prepare(
            "INSERT INTO speaker_embedding(speaker_id, values_json)
             VALUES (?1, ?2)
             ON CONFLICT(speaker_id) DO UPDATE SET
                values_json = excluded.values_json,
                updated_at = CURRENT_TIMESTAMP",
        )?;
        for (speaker_id, values) in embedding_updates {
            let values_json = serde_json::to_string(values)
                .map_err(|error| StorageError::Metadata(error.to_string()))?;
            embedding_statement.execute(params![speaker_id, values_json])?;
        }
        drop(embedding_statement);

        tx.execute(
            "DELETE FROM speaker_merge_proposal WHERE status = 'pending'",
            [],
        )?;
        let mut proposal_statement = tx.prepare(
            "INSERT INTO speaker_merge_proposal(
                id, left_speaker_id, right_speaker_id, similarity, evidence, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
        )?;
        for proposal in proposals {
            proposal_statement.execute(params![
                proposal.id,
                proposal.left_speaker_id,
                proposal.right_speaker_id,
                proposal.similarity,
                proposal.evidence,
            ])?;
        }
        drop(proposal_statement);

        Self::log_operation_in(
            &tx,
            revision,
            "speaker_diarize",
            &serde_json::json!({
                "asset_id": asset_id,
                "speaker_count": identities.len(),
                "segment_count": segments.len(),
                "word_assignment_count": assignments.len(),
                "merge_proposal_count": proposals.len(),
            }),
        )?;
        Self::capture_snapshot_in(&tx, revision)?;
        tx.commit()?;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::{Connection, params};
    use sha2::{Digest, Sha256};

    use super::{MIGRATION_V1, MIGRATION_V2, MIGRATION_V3, MIGRATION_V4, MIGRATIONS, ProjectStore};
    use crate::rational::FrameRate;

    const HISTORICAL_MIGRATION_SHA256: &[(u32, &str)] = &[
        (
            1,
            "4fbc6f32be347c58831f7b4c2e50dd3db6fa4894bbe52a249ade768b48f74536",
        ),
        (
            2,
            "e8e994e2ea04c45a78dd78d79ba4db8ce0ca7ef197cc9b6b29fc7a87ab0d58ca",
        ),
        (
            3,
            "ef2699c088aedcc28d6b182ac171c1639985cdefec4282acb6facf0919feea76",
        ),
        (
            4,
            "e5b57d35e5861b75a2f0233c221d8b88c3f51382175725e805f150ea0889600b",
        ),
        (
            5,
            "24a71143282bb0e9be77a9a19627d76677fbf60ccb59d04c223dac08f6e8377d",
        ),
        (
            6,
            "a7b12e92bd90c3b04da5f8817de938f8cc22fdd9124861de60b7a17b19c5ea98",
        ),
        (
            7,
            "8de8d01911eef3e5cfec8e804384a14989fa18b822de0f63a0fb0f543c1d2b4c",
        ),
        (
            8,
            "04a5120ae79cbcd3c90cbacf6b469ffac7735612248e7cd56e42c0cd006bbb57",
        ),
        (
            9,
            "152426c47350cdf30e8c58fbc2f1c5532d60e344a44f29c8bd73aefdb9301d0c",
        ),
        (
            10,
            "78f893e53a207c48fbb138dad088b03cac4ccbddae2bc166774254f071a26ab7",
        ),
    ];

    fn assert_historical_migration_fingerprints() {
        assert_eq!(MIGRATIONS.len(), HISTORICAL_MIGRATION_SHA256.len());
        for ((version, sql), (expected_version, expected_sha256)) in
            MIGRATIONS.iter().zip(HISTORICAL_MIGRATION_SHA256)
        {
            assert_eq!(version, expected_version, "migration order changed");
            assert_eq!(
                format!("{:x}", Sha256::digest(sql.as_bytes())),
                *expected_sha256,
                "migration v{version} text changed"
            );
        }
    }

    fn temp_db_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("double-love-{label}-{unique}.sqlite"))
    }

    fn construct_historical_schema(path: &std::path::Path, target_version: u32) {
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
        let connection = Connection::open(path).expect("historical fixture opens");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );",
            )
            .expect("schema migration ledger");
        for (version, sql) in MIGRATIONS
            .iter()
            .copied()
            .take_while(|(version, _)| *version <= target_version)
        {
            connection
                .execute_batch(sql)
                .unwrap_or_else(|error| panic!("schema {version} fixture applies: {error}"));
            connection
                .execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    [version],
                )
                .expect("fixture version records");
        }
    }

    #[test]
    fn every_historical_schema_constructs_and_migrates_idempotently_to_v10() {
        assert_historical_migration_fingerprints();

        let root = temp_db_path("schema-history").with_extension("");
        let expected_versions: Vec<u32> = (1..=10).collect();

        for source_version in 1..=10 {
            let path = root
                .join(format!("schema-v{source_version}"))
                .join(".doublelove/project.sqlite");
            construct_historical_schema(&path, source_version);

            drop(ProjectStore::open(&path).expect("historical fixture migrates"));
            drop(ProjectStore::open(&path).expect("migration is idempotent"));

            let connection = Connection::open(&path).expect("migrated fixture opens");
            let versions = connection
                .prepare("SELECT version FROM schema_migrations ORDER BY version")
                .expect("versions query")
                .query_map([], |row| row.get(0))
                .expect("versions read")
                .collect::<Result<Vec<u32>, _>>()
                .expect("versions decode");
            assert_eq!(
                versions, expected_versions,
                "source schema {source_version}"
            );
            let current: u32 = connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })
                .expect("current version");
            assert_eq!(current, 10, "source schema {source_version}");
        }

        fs::remove_dir_all(root).expect("historical fixtures are removed");
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
    fn migrates_a_v1_store_to_the_current_schema_and_is_idempotent() {
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
        assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        for table in [
            "media_asset",
            "transcript_word",
            "edit_operation",
            "export_artifact",
            "main_track_clip",
            "speaker_identity",
            "speaker_segment",
            "speaker_merge_proposal",
            "speaker_embedding",
            "project_snapshot",
            "transcript_run",
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
        // 词序唯一约束来自迁移本身，而不是事后补丁：同一转录版本内的 ordinal 必须唯一。
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
                "INSERT INTO transcript_run(id, asset_id, model, language, status)
                 VALUES ('run-a1', 'a1', 'test', 'zh', 'running')",
                [],
            )
            .expect("transcript run inserts");
        connection
            .execute(
                "INSERT INTO transcript_word(
                    word_id, asset_id, ordinal, raw_text, display_text, start_sample, end_sample, run_id
                 ) VALUES ('w1', 'a1', 0, '你', '你', 0, 4800, 'run-a1')",
                [],
            )
            .expect("first word inserts");
        let duplicate = connection.execute(
            "INSERT INTO transcript_word(
                word_id, asset_id, ordinal, raw_text, display_text, start_sample, end_sample, run_id
             ) VALUES ('w2', 'a1', 0, '好', '好', 4800, 9600, 'run-a1')",
            [],
        );
        assert!(duplicate.is_err(), "UNIQUE(run_id, ordinal) must hold");
        drop(connection);

        // 再次打开不得重复迁移、不得报错。
        let store = ProjectStore::open(&path).expect("reopen is idempotent");
        drop(store);

        fs::remove_file(path).expect("temporary database is removed");
    }

    #[test]
    fn v5_moves_existing_words_and_edits_into_a_legacy_run() {
        let path = temp_db_path("migrate-v5");
        let connection = Connection::open(&path).expect("v4 database opens");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );",
            )
            .expect("schema migrations table");
        for (version, migration) in [
            (1, MIGRATION_V1),
            (2, MIGRATION_V2),
            (3, MIGRATION_V3),
            (4, MIGRATION_V4),
        ] {
            connection
                .execute_batch(migration)
                .expect("legacy migration applies");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    [version],
                )
                .expect("legacy migration records");
        }
        connection
            .execute(
                "INSERT INTO media_asset(
                    id, kind, original_path, display_name, duration_samples,
                    audio_sample_rate, fps_num, fps_den, video_timebase, is_ntsc, ffprobe_json
                 ) VALUES ('a1', 'video', '/tmp/legacy.mov', 'legacy.mov', 480000,
                    48000, 25, 1, 25, 0, '{}')",
                [],
            )
            .expect("legacy asset");
        connection
            .execute(
                "INSERT INTO transcript_word(
                    word_id, asset_id, ordinal, raw_text, display_text, start_sample, end_sample
                 ) VALUES ('w1', 'a1', 0, '旧', '旧', 0, 4800)",
                [],
            )
            .expect("legacy word");
        connection
            .execute("INSERT INTO revisions(operation) VALUES ('edit_omit')", [])
            .expect("legacy revision");
        connection
            .execute(
                "INSERT INTO edit_operation(
                    id, revision, asset_id, type, behavior, start_ordinal, end_ordinal,
                    handles_before_ms, handles_after_ms
                 ) VALUES ('op1', 1, 'a1', 'omit', 'ripple_av', 0, 0, 120, 120)",
                [],
            )
            .expect("legacy omit");
        drop(connection);

        let store = ProjectStore::open(&path).expect("v5 migration succeeds");
        assert_eq!(
            store.active_transcript_run_id("a1").expect("active run"),
            Some("legacy:a1".to_string())
        );
        assert_eq!(store.transcript_words("a1").expect("words").len(), 1);
        assert_eq!(
            store.active_omit_operations("a1").expect("omits").len(),
            1,
            "old omit stays bound to the migrated legacy run"
        );
        drop(store);
        fs::remove_file(path).expect("temporary database is removed");
    }

    #[test]
    fn main_track_operations_keep_a_dense_order_and_revision_log() {
        let path = temp_db_path("main-track");
        let store = ProjectStore::open(&path).expect("store opens");
        for id in ["a", "b"] {
            store
                .connection
                .execute(
                    "INSERT INTO media_asset(
                        id, kind, original_path, display_name, duration_samples,
                        audio_sample_rate, fps_num, fps_den, video_timebase, is_ntsc, ffprobe_json
                     ) VALUES (?1, 'video', ?2, ?3, 480000, 48000, 25, 1, 25, 0, '{}')",
                    params![id, format!("/tmp/{id}.mov"), format!("{id}.mov")],
                )
                .expect("asset inserts");
        }
        store
            .append_main_track_clip("c1", "a", 0, 100)
            .expect("first clip");
        store
            .append_main_track_clip("c2", "b", 0, 100)
            .expect("second clip");
        store
            .move_main_track_clip("c2", Some("c1"))
            .expect("move before");
        let (left, right) = store.split_main_track_clip("c1", "c3", 40).expect("split");
        assert_eq!((left.source_in_frame, left.source_out_frame), (0, 40));
        assert_eq!((right.source_in_frame, right.source_out_frame), (40, 100));
        store.remove_main_track_clip("c2").expect("remove");
        let clips = store.main_track_clips().expect("clips list");
        assert_eq!(
            clips
                .iter()
                .map(|clip| clip.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "c3"]
        );
        assert_eq!(
            clips
                .iter()
                .map(|clip| clip.order_index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(store.revision().expect("revision") >= 5);
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn history_restores_main_track_and_project_level_visual_state_as_a_new_revision() {
        let path = temp_db_path("history");
        let store = ProjectStore::open(&path).expect("store opens");
        for id in ["a", "b"] {
            store
                .connection
                .execute(
                    "INSERT INTO media_asset(
                        id, kind, original_path, display_name, duration_samples,
                        audio_sample_rate, fps_num, fps_den, video_timebase, is_ntsc, ffprobe_json
                     ) VALUES (?1, 'video', ?2, ?3, 480000, 48000, 25, 1, 25, 0, '{}')",
                    params![id, format!("/tmp/{id}-history.mov"), format!("{id}.mov")],
                )
                .expect("asset inserts");
        }
        store
            .append_main_track_clip("c1", "a", 0, 100)
            .expect("first clip");
        store
            .set_output_rate(FrameRate::Fps30)
            .expect("output rate saves");
        let canvas = crate::contracts::CanvasSpec {
            width: 1080,
            height: 1920,
            ..Default::default()
        };
        let snapshot_revision = store.set_canvas_spec(&canvas).expect("canvas saves");
        store
            .append_main_track_clip("c2", "b", 0, 100)
            .expect("second clip");
        store
            .set_output_rate(FrameRate::Fps25)
            .expect("output rate changes");
        assert_eq!(store.main_track_clips().expect("clips").len(), 2);
        assert!(
            store
                .revision_history(20)
                .expect("history")
                .iter()
                .any(|entry| entry.revision == snapshot_revision && entry.restorable)
        );

        let restored = store
            .restore_revision(snapshot_revision)
            .expect("history restores");
        assert!(restored > snapshot_revision);
        assert_eq!(store.main_track_clips().expect("clips").len(), 1);
        assert_eq!(store.main_track_clips().expect("clips")[0].id, "c1");
        assert_eq!(store.canvas_spec().expect("canvas"), canvas);
        assert_eq!(
            store.output_rate().expect("output rate"),
            Some(FrameRate::Fps30)
        );
        assert_eq!(
            store.revision_history(1).expect("history")[0].operation,
            "history_restore"
        );
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn output_rate_can_return_to_first_main_track_default() {
        let path = temp_db_path("output-rate");
        let store = ProjectStore::open(&path).expect("store");
        store
            .set_output_rate(FrameRate::Fps60Ntsc)
            .expect("output rate saves");
        assert_eq!(
            store.output_rate().expect("explicit output rate"),
            Some(FrameRate::Fps60Ntsc)
        );
        store.clear_output_rate().expect("output rate clears");
        assert_eq!(store.output_rate().expect("default output rate"), None);
        assert_eq!(
            store.revision_history(1).expect("history")[0].operation,
            "output_rate_clear"
        );
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn legacy_history_snapshot_does_not_clear_a_newer_output_rate() {
        let path = temp_db_path("legacy-output-rate-history");
        let store = ProjectStore::open(&path).expect("store");
        let legacy_revision = store
            .set_output_rate(FrameRate::Fps30)
            .expect("first output rate");
        let legacy_snapshot = serde_json::json!({
            "main_track": [],
            "canvas": crate::contracts::CanvasSpec::default(),
            "subtitle_style": crate::contracts::SubtitleStyle::default(),
            "active_omits": [],
        });
        store
            .connection
            .execute(
                "UPDATE project_snapshot SET snapshot_json = ?2 WHERE revision = ?1",
                params![legacy_revision, legacy_snapshot.to_string()],
            )
            .expect("replace with legacy snapshot");
        store
            .set_output_rate(FrameRate::Fps60)
            .expect("newer output rate");
        store
            .restore_revision(legacy_revision)
            .expect("restore legacy snapshot");
        assert_eq!(
            store.output_rate().expect("current output rate"),
            Some(FrameRate::Fps60)
        );
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn history_restores_speaker_names_merges_segments_and_word_assignments() {
        let path = temp_db_path("speaker-history");
        let store = ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&super::NewMediaAsset {
                id: "a".to_string(),
                kind: "video".to_string(),
                original_path: "/tmp/speaker-history.mov".to_string(),
                display_name: "speaker-history.mov".to_string(),
                duration_samples: 48_000,
                audio_sample_rate: 48_000,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: None,
                source_tc_is_drop_frame: false,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        store
            .insert_transcript_words(&[super::NewTranscriptWord {
                word_id: "w".to_string(),
                asset_id: "a".to_string(),
                ordinal: 0,
                raw_text: "你好".to_string(),
                display_text: "你好".to_string(),
                language: Some("zh".to_string()),
                start_sample: 0,
                end_sample: 24_000,
                confidence: Some(0.99),
            }])
            .expect("word");
        for (id, name) in [("s1", "说话人 1"), ("s2", "说话人 2")] {
            store
                .upsert_speaker_identity(&crate::contracts::SpeakerIdentity {
                    id: id.to_string(),
                    display_name: name.to_string(),
                    aliases: Vec::new(),
                    color: "#3366FF".to_string(),
                    confirmed: false,
                })
                .expect("identity");
        }
        store
            .replace_speaker_segments(
                "a",
                &[crate::contracts::SpeakerSegment {
                    id: "seg".to_string(),
                    asset_id: "a".to_string(),
                    speaker_id: "s2".to_string(),
                    start_sample: 0,
                    end_sample: 24_000,
                    confidence: Some(0.9),
                }],
            )
            .expect("segment");
        store
            .set_word_speaker_assignments(
                "a",
                &[(
                    "w".to_string(),
                    vec![crate::contracts::SpeakerAssignment {
                        speaker_id: "s2".to_string(),
                        confidence: Some(0.9),
                        evidence: "test".to_string(),
                    }],
                )],
            )
            .expect("assignment");
        store
            .confirm_speaker_name("s1", "主持人")
            .expect("name confirmation");
        let before_merge_revision = store.revision().expect("snapshot revision");

        store
            .merge_speaker_identities("s1", "s2")
            .expect("merge speakers");
        assert_eq!(
            store.speaker_identities().expect("merged speakers").len(),
            1
        );
        assert_eq!(
            store.speaker_segments("a").expect("merged segments")[0].speaker_id,
            "s1"
        );
        assert_eq!(
            store.transcript_words("a").expect("merged words")[0].speaker_assignments[0].speaker_id,
            "s1"
        );

        store
            .restore_revision(before_merge_revision)
            .expect("history restores speakers");
        let restored = store.speaker_identities().expect("restored identities");
        assert_eq!(restored.len(), 2);
        let host = restored
            .iter()
            .find(|speaker| speaker.id == "s1")
            .expect("host");
        assert_eq!(host.display_name, "主持人");
        assert!(!host.aliases.iter().any(|alias| alias == "说话人 2"));
        assert_eq!(
            store.speaker_segments("a").expect("restored segments")[0].speaker_id,
            "s2"
        );
        assert_eq!(
            store.transcript_words("a").expect("restored words")[0].speaker_assignments[0]
                .speaker_id,
            "s2"
        );
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn persists_project_subtitle_style_and_speaker_assignments() {
        let path = temp_db_path("speaker-style");
        let store = ProjectStore::open(&path).expect("store opens");
        store
            .connection
            .execute(
                "INSERT INTO media_asset(
                    id, kind, original_path, display_name, duration_samples,
                    audio_sample_rate, fps_num, fps_den, video_timebase, is_ntsc, ffprobe_json
                 ) VALUES ('a', 'video', '/tmp/a.mov', 'a.mov', 480000, 48000, 25, 1, 25, 0, '{}')",
                [],
            )
            .expect("asset");
        store
            .insert_transcript_words(&[super::NewTranscriptWord {
                word_id: "w".to_string(),
                asset_id: "a".to_string(),
                ordinal: 0,
                raw_text: "你好".to_string(),
                display_text: "你好".to_string(),
                language: Some("zh".to_string()),
                start_sample: 0,
                end_sample: 4800,
                confidence: Some(0.9),
            }])
            .expect("word");
        let identity = crate::contracts::SpeakerIdentity {
            id: "s1".to_string(),
            display_name: "采访者".to_string(),
            aliases: vec!["主持人".to_string()],
            color: "#3366FF".to_string(),
            confirmed: true,
        };
        store.upsert_speaker_identity(&identity).expect("speaker");
        store
            .set_word_speaker_assignments(
                "a",
                &[(
                    "w".to_string(),
                    vec![crate::contracts::SpeakerAssignment {
                        speaker_id: "s1".to_string(),
                        confidence: Some(0.9),
                        evidence: "manual".to_string(),
                    }],
                )],
            )
            .expect("assignment");
        let style = crate::contracts::SubtitleStyle {
            font_size: 48.0,
            ..Default::default()
        };
        store.set_subtitle_style(&style).expect("style");
        assert_eq!(
            store.speaker_identities().expect("identities"),
            vec![identity]
        );
        assert_eq!(
            store.transcript_words("a").expect("words")[0].speaker_assignments[0].speaker_id,
            "s1"
        );
        assert_eq!(store.subtitle_style().expect("style read").font_size, 48.0);
        drop(store);
        fs::remove_file(path).ok();
    }

    #[test]
    fn confirmed_speaker_merge_rewrites_segments_and_active_word_assignments() {
        let path = temp_db_path("speaker-merge");
        let store = ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&super::NewMediaAsset {
                id: "a".to_string(),
                kind: "video".to_string(),
                original_path: "/tmp/speaker-merge.mov".to_string(),
                display_name: "speaker-merge.mov".to_string(),
                duration_samples: 48_000,
                audio_sample_rate: 48_000,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: None,
                source_tc_is_drop_frame: false,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        store
            .insert_transcript_words(&[super::NewTranscriptWord {
                word_id: "w".to_string(),
                asset_id: "a".to_string(),
                ordinal: 0,
                raw_text: "你好".to_string(),
                display_text: "你好".to_string(),
                language: Some("zh".to_string()),
                start_sample: 0,
                end_sample: 24_000,
                confidence: Some(0.99),
            }])
            .expect("word");
        for (id, name) in [("s1", "访谈者"), ("s2", "嘉宾")] {
            store
                .upsert_speaker_identity(&crate::contracts::SpeakerIdentity {
                    id: id.to_string(),
                    display_name: name.to_string(),
                    aliases: Vec::new(),
                    color: "#3366FF".to_string(),
                    confirmed: false,
                })
                .expect("identity");
        }
        store
            .replace_speaker_segments(
                "a",
                &[crate::contracts::SpeakerSegment {
                    id: "seg".to_string(),
                    asset_id: "a".to_string(),
                    speaker_id: "s2".to_string(),
                    start_sample: 0,
                    end_sample: 24_000,
                    confidence: Some(0.9),
                }],
            )
            .expect("segment");
        store
            .set_word_speaker_assignments(
                "a",
                &[(
                    "w".to_string(),
                    vec![crate::contracts::SpeakerAssignment {
                        speaker_id: "s2".to_string(),
                        confidence: Some(0.9),
                        evidence: "test".to_string(),
                    }],
                )],
            )
            .expect("assignment");
        store.merge_speaker_identities("s1", "s2").expect("merge");
        assert_eq!(
            store.speaker_segments("a").expect("segments")[0].speaker_id,
            "s1"
        );
        assert_eq!(
            store.transcript_words("a").expect("words")[0].speaker_assignments[0].speaker_id,
            "s1"
        );
        assert_eq!(
            store
                .speaker_identities()
                .expect("visible identities")
                .len(),
            1
        );
        drop(store);
        fs::remove_file(path).ok();
    }
}
