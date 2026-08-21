//! 项目级导出编排：唯一 TimelineIR v2 → 字幕 Cue → ASS / 双端共用 XMEML。
//!
//! 预览只计算，不落盘；Apply 通过临时文件和 rename 原子写入，失败不会留下貌似成功的成品。

use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::contracts::{CompatibilityReport, ProjectExportPreview};
use crate::export::xmeml_v2::{XmemlV2Input, export_xmeml_v2};
use crate::storage::{ProjectStore, StorageError};
use crate::{
    OperationResult, OutputArtifact, apply_speaker_names, build_subtitle_cues,
    compile_project_timeline, export_ass,
};

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

fn compatibility() -> Vec<CompatibilityReport> {
    let preserved = vec![
        "多源主轨顺序、源入点/出点与输出切点".to_string(),
        "源文件路径、原生帧率、音频采样率与源时间码起点".to_string(),
        "字幕文字与输出时间（单独的可编辑 Text 轨）".to_string(),
        "项目输出分辨率与帧率".to_string(),
    ];
    vec![
        CompatibilityReport {
            target: "Premiere Pro (XMEML)".to_string(),
            preserved: preserved.clone(),
            limitations: vec![
                "字幕外观以 ASS / 烧录 MP4 为准；XMEML 只承诺文字与时间。".to_string(),
                "全局画布的 contain/cover、裁切、旋转与不透明度未保证可被 Premiere 等价还原。"
                    .to_string(),
                "29.97/59.94 的 DF/NDF 时间码会随源素材写入；仍须在 Premiere 实机验收混合帧率与重连。"
                    .to_string(),
            ],
        },
        CompatibilityReport {
            target: "DaVinci Resolve (XMEML)".to_string(),
            preserved,
            limitations: vec![
                "字幕外观以 ASS / 烧录 MP4 为准；XMEML 只承诺文字与时间。".to_string(),
                "Resolve 对 XMEML Text generator、混合帧率和重连行为必须在实机导入验收；失败时应启用 FCPXML Adapter。"
                    .to_string(),
                "全局画布的 contain/cover、裁切、旋转与不透明度未保证可被 Resolve 等价还原。"
                    .to_string(),
            ],
        },
    ]
}

/// 编译一次项目级导出预览。ASS、MP4 和 NLE 都必须从此对象继续写出，不能自行重算。
pub fn preview_project_export(
    store: &ProjectStore,
    name: &str,
) -> OperationResult<ProjectExportPreview> {
    let compiled = compile_project_timeline(store, name);
    let mut result = OperationResult::<ProjectExportPreview> {
        status: compiled.status,
        revision: compiled.revision,
        data: None,
        counts: compiled.counts,
        diagnostics: compiled.diagnostics,
        outputs: compiled.outputs,
    };
    let Some(timeline) = compiled.data else {
        return result;
    };
    let mut source_words = HashMap::new();
    for source in &timeline.sources {
        match store.transcript_words(&source.asset_id) {
            Ok(words) => {
                source_words.insert(source.asset_id.clone(), words);
            }
            Err(error) => return storage_failure(error),
        }
    }
    let style = match store.subtitle_style() {
        Ok(style) => style,
        Err(error) => return storage_failure(error),
    };
    let speaker_names = match store.speaker_identities() {
        Ok(identities) => identities
            .into_iter()
            .map(|identity| (identity.id, identity.display_name))
            .collect::<HashMap<_, _>>(),
        Err(error) => return storage_failure(error),
    };
    let mut subtitle_cues = build_subtitle_cues(&timeline, &source_words, &style);
    apply_speaker_names(&mut subtitle_cues, &speaker_names);
    result.counts.total = timeline.clips.len() as u64;
    result.counts.processed = timeline.clips.len() as u64;
    result.data = Some(ProjectExportPreview {
        timeline,
        subtitle_cues,
        compatibility: compatibility(),
    });
    result
}

fn write_atomically(target: &Path, content: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "导出路径没有父目录。".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("无法创建导出目录：{error}"))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("export"),
        Uuid::new_v4()
    ));
    std::fs::write(&temp, content).map_err(|error| format!("无法写入临时导出文件：{error}"))?;
    if let Err(error) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("无法原子替换导出文件：{error}"));
    }
    Ok(())
}

fn apply_text_export(
    store: &ProjectStore,
    name: &str,
    target: &Path,
    kind: &str,
    render: impl FnOnce(&ProjectExportPreview, &crate::contracts::SubtitleStyle) -> String,
) -> OperationResult<ProjectExportPreview> {
    let mut result = preview_project_export(store, name);
    let Some(preview) = result.data.as_ref() else {
        return result;
    };
    let style = match store.subtitle_style() {
        Ok(style) => style,
        Err(error) => return storage_failure(error),
    };
    let text = render(preview, &style);
    if let Err(error) = write_atomically(target, text.as_bytes()) {
        return OperationResult::failed("EXPORT_WRITE_FAILED", error);
    }
    let sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));
    let source_asset_id = match preview.timeline.clips.first() {
        Some(clip) => &clip.source_asset_id,
        None => return OperationResult::failed("TIMELINE_EMPTY", "主轨没有可导出的片段。"),
    };
    let path = target.to_string_lossy().into_owned();
    match store.apply_export_artifact(
        &Uuid::new_v4().to_string(),
        source_asset_id,
        kind,
        &path,
        &sha256,
    ) {
        Ok(revision) => result.revision = Some(revision),
        Err(error) => return storage_failure(error),
    }
    result.outputs.push(OutputArtifact {
        kind: kind.to_string(),
        path,
        sha256: Some(sha256),
    });
    result
}

/// 写入 Premiere / Resolve 共用的 XMEML。兼容性边界必须随预览一起展示给用户。
pub fn export_project_xmeml_to(
    store: &ProjectStore,
    name: &str,
    target: &Path,
) -> OperationResult<ProjectExportPreview> {
    apply_text_export(
        store,
        name,
        target,
        "premiere_resolve_xmeml",
        |preview, _| {
            export_xmeml_v2(&XmemlV2Input {
                ir: &preview.timeline,
                cues: &preview.subtitle_cues,
            })
        },
    )
}

/// 写出完整项目级字幕样式的 ASS。
pub fn export_project_ass_to(
    store: &ProjectStore,
    name: &str,
    target: &Path,
) -> OperationResult<ProjectExportPreview> {
    apply_text_export(store, name, target, "ass", |preview, style| {
        export_ass(&preview.timeline, style, &preview.subtitle_cues)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rational::FrameRate;
    use crate::storage::NewMediaAsset;

    fn store() -> (ProjectStore, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "double-love-project-export-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = ProjectStore::open(&path).expect("store");
        store
            .insert_media_asset(&NewMediaAsset {
                id: "a".to_string(),
                kind: "video".to_string(),
                original_path: "/tmp/a.mov".to_string(),
                display_name: "a.mov".to_string(),
                duration_samples: 480_000,
                audio_sample_rate: 48_000,
                fps_num: 25,
                fps_den: 1,
                video_timebase: 25,
                is_ntsc: false,
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: Some(0),
                source_tc_is_drop_frame: false,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset");
        store
            .append_main_track_clip("c", "a", 0, 250)
            .expect("clip");
        (store, path)
    }

    #[test]
    fn preview_and_ass_apply_share_one_timeline() {
        let (store, db) = store();
        let preview = preview_project_export(&store, "rough cut");
        assert_eq!(preview.status, crate::OperationStatus::Success);
        assert_eq!(
            preview.data.as_ref().expect("data").timeline.rate,
            FrameRate::Fps25
        );
        assert_eq!(preview.data.as_ref().expect("data").compatibility.len(), 2);
        let dir = std::env::temp_dir().join(format!("double-love-ass-{}", Uuid::new_v4()));
        let target = dir.join("rough-cut.ass");
        let applied = export_project_ass_to(&store, "rough cut", &target);
        assert_eq!(applied.status, crate::OperationStatus::Success);
        assert!(target.is_file());
        assert_eq!(applied.outputs[0].kind, "ass");
        drop(store);
        std::fs::remove_file(db).ok();
        std::fs::remove_dir_all(dir).ok();
    }
}
