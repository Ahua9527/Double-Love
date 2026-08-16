//! 粗剪导出编排：compile → XMEML →（apply 时）写文件 + sha256 + export_artifact。
//! Preview 先于 Apply（PRD 不变量）：preview 不落盘、不写库，返回同一份 IR。

use std::path::Path;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::compile::{CompileOptions, compile_rough_cut};
use crate::contracts::TimelineIR;
use crate::export::xmeml::{XmemlInput, export_xmeml};
use crate::storage::{ProjectStore, StorageError};
use crate::{OperationResult, OutputArtifact};

/// 导出结果：IR 必有（preview 也返回）；文件与 sha256 仅 apply 后有。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ExportOutcome {
    pub ir: TimelineIR,
    pub artifact_path: Option<String>,
    pub sha256: Option<String>,
}

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

/// 编译并（可选）落盘。apply=false → preview：只算不写。
pub fn export_rough_cut(
    store: &ProjectStore,
    asset_id: &str,
    exports_dir: &Path,
    apply: bool,
) -> OperationResult<ExportOutcome> {
    let asset = match store.media_asset(asset_id) {
        Ok(Some(asset)) => asset,
        Ok(None) => {
            return OperationResult::failed(
                "MEDIA_ASSET_MISSING",
                format!("资产不存在：{asset_id}"),
            );
        }
        Err(error) => return storage_failure(error),
    };
    let words = match store.transcript_words(asset_id) {
        Ok(words) => words,
        Err(error) => return storage_failure(error),
    };
    let omits = match store.active_omit_operations(asset_id) {
        Ok(omits) => omits,
        Err(error) => return storage_failure(error),
    };

    let stem = asset
        .display_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(&asset.display_name)
        .to_string();
    let name = format!("{stem} ROUGH CUT");
    let compiled = compile_rough_cut(&asset, &words, &omits, &name, &CompileOptions::default());
    let mut result = OperationResult::<ExportOutcome> {
        status: compiled.status,
        revision: None,
        data: None,
        counts: compiled.counts,
        diagnostics: compiled.diagnostics,
        outputs: Vec::new(),
    };
    let Some(ir) = compiled.data else {
        return result; // 编译失败（如 ROUGH_CUT_EMPTY）：诊断原样透出
    };

    if !apply {
        result.data = Some(ExportOutcome {
            ir,
            artifact_path: None,
            sha256: None,
        });
        return result;
    }

    let xml = export_xmeml(&XmemlInput {
        ir: &ir,
        source_path: &asset.original_path,
        file_name: &asset.display_name,
        width: asset.width,
        height: asset.height,
        audio_sample_rate: asset.audio_sample_rate,
        audio_channels: asset.audio_channels,
        source_tc_start_frame: asset.source_tc_start_frame,
    });
    if let Err(error) = std::fs::create_dir_all(exports_dir) {
        return OperationResult::failed(
            "EXPORT_WRITE_FAILED",
            format!("无法创建导出目录：{error}"),
        );
    }
    let artifact_path = exports_dir.join(format!("{stem}_ROUGH_CUT.xml"));
    if let Err(error) = std::fs::write(&artifact_path, &xml) {
        return OperationResult::failed(
            "EXPORT_WRITE_FAILED",
            format!("写入导出文件失败：{error}"),
        );
    }
    let sha256 = format!("{:x}", Sha256::digest(xml.as_bytes()));
    let path_string = artifact_path.to_string_lossy().into_owned();

    let artifact_id = Uuid::new_v4().to_string();
    match store.apply_export_artifact(
        &artifact_id,
        asset_id,
        "premiere_xmeml",
        &path_string,
        &sha256,
    ) {
        Ok(revision) => result.revision = Some(revision),
        Err(error) => return storage_failure(error),
    }
    result.outputs.push(OutputArtifact {
        kind: "premiere_xmeml".to_string(),
        path: path_string.clone(),
        sha256: Some(sha256.clone()),
    });
    result.data = Some(ExportOutcome {
        ir,
        artifact_path: Some(path_string),
        sha256: Some(sha256),
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::omit_words;
    use crate::storage::{NewMediaAsset, NewTranscriptWord};

    fn fixture(word_count: i64) -> (ProjectStore, std::path::PathBuf) {
        // macOS 系统时钟只到微秒，并行测试可能同刻：纳秒 + pid + 原子序号三重去重。
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let sequence = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "double-love-export-{}-{unique}-{sequence}.sqlite",
            std::process::id()
        ));
        let store = ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&NewMediaAsset {
                id: "a1".to_string(),
                kind: "video".to_string(),
                original_path: "/tmp/合成素材 01.mp4".to_string(),
                display_name: "合成素材 01.mp4".to_string(),
                duration_samples: 480_000,
                audio_sample_rate: 48_000,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: None,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        let words: Vec<NewTranscriptWord> = (0..word_count)
            .map(|i| NewTranscriptWord {
                word_id: format!("w{i}"),
                asset_id: "a1".to_string(),
                ordinal: i,
                raw_text: format!("词{i}"),
                display_text: format!("词{i}"),
                language: Some("zh".to_string()),
                start_sample: i * 48_000,
                end_sample: i * 48_000 + 40_000,
                confidence: Some(0.99),
            })
            .collect();
        store.insert_transcript_words(&words).expect("words");
        (store, path)
    }

    #[test]
    fn preview_computes_without_writing_and_apply_persists() {
        let (store, db) = fixture(6);
        let exports = std::env::temp_dir().join(format!("dl-exports-{}", std::process::id()));
        omit_words(&store, "a1", 2, 3, 120, 120);

        // preview：有 IR，无文件、无 artifact
        let preview = export_rough_cut(&store, "a1", &exports, false);
        assert_eq!(preview.status, crate::OperationStatus::Success);
        let outcome = preview.data.as_ref().expect("preview data");
        assert_eq!(outcome.ir.clips.len(), 2);
        assert!(outcome.artifact_path.is_none());
        assert!(!exports.exists(), "preview 不得落盘");
        assert_eq!(store.revision().expect("rev"), 1, "preview 不得开修订");

        // apply：文件存在、sha256 与内容一致、artifact 落库
        let applied = export_rough_cut(&store, "a1", &exports, true);
        let outcome = applied.data.as_ref().expect("apply data");
        let path = outcome.artifact_path.clone().expect("path");
        let bytes = std::fs::read(&path).expect("artifact file");
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            outcome.sha256.clone().expect("sha")
        );
        let xml = String::from_utf8(bytes).expect("utf8");
        assert!(xml.contains("<!DOCTYPE xmeml>"));
        assert!(xml.matches("<clipitem id=").count() == 4, "2 视频 + 2 音频");
        assert!(applied.revision.expect("revision") >= 2);
        assert_eq!(applied.outputs.len(), 1);

        drop(store);
        std::fs::remove_file(db).ok();
        std::fs::remove_dir_all(exports).ok();
    }

    #[test]
    fn empty_rough_cut_blocks_apply() {
        let (store, db) = fixture(3);
        omit_words(&store, "a1", 0, 2, 120, 120);
        let exports = std::env::temp_dir().join(format!("dl-exports-empty-{}", std::process::id()));
        let result = export_rough_cut(&store, "a1", &exports, true);
        assert_eq!(result.status, crate::OperationStatus::Failed);
        assert_eq!(result.diagnostics[0].code, "ROUGH_CUT_EMPTY");
        assert!(result.diagnostics[0].blocks_export);
        assert!(!exports.exists(), "失败不得落盘");
        drop(store);
        std::fs::remove_file(db).ok();
    }
}
