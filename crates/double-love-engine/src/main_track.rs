//! 项目级主轨操作。这里负责把 SQLite 的素材/片段记录变成 TimelineIR v2，UI 与 CLI
//! 都不得自行拼接源时间、输出时间或顺序。文字 omit 会先在各源自身时间域编译成可恢复
//! 的 SourceCut，再与用户排好的主轨片段求交；因此所有下游消费者只看同一份结果。

use std::collections::HashMap;

use uuid::Uuid;

use crate::OperationResult;
use crate::compile::{CompileOptions, compile_rough_cut};
use crate::contracts::{MainTrackClip, SourceCut, TimelineIRv2, TimelineSource};
use crate::rational::{FrameRate, Rational};
use crate::storage::{MediaAssetRow, ProjectStore, StorageError};
use crate::timeline::compile_main_track;

fn storage_failure<T>(error: StorageError) -> OperationResult<T> {
    OperationResult::failed("STORAGE_ERROR", error.to_string())
}

fn source_from_row(row: MediaAssetRow) -> Result<TimelineSource, String> {
    let rate = FrameRate::from_rational(&Rational::new(row.fps_num, row.fps_den))
        .ok_or_else(|| format!("素材 {} 的帧率不受支持。", row.id))?;
    let duration_frames = crate::samples_to_frame(
        row.duration_samples,
        rate,
        row.audio_sample_rate,
        crate::Round::Floor,
    );
    Ok(TimelineSource {
        asset_id: row.id,
        display_name: row.display_name,
        original_path: row.original_path,
        rate,
        source_duration_frames: duration_frames,
        audio_sample_rate: row.audio_sample_rate,
        audio_channels: row.audio_channels,
        width: row.width,
        height: row.height,
        source_tc_start_frame: row.source_tc_start_frame,
        source_tc_is_drop_frame: row.source_tc_is_drop_frame,
    })
}

fn require_source(
    store: &ProjectStore,
    asset_id: &str,
) -> Result<TimelineSource, Box<OperationResult<MainTrackClip>>> {
    let row = match store.active_media_asset(asset_id) {
        Ok(Some(row)) => row,
        Ok(None) => {
            return Err(Box::new(OperationResult::failed(
                "MEDIA_ASSET_MISSING",
                format!("素材不存在：{asset_id}"),
            )));
        }
        Err(error) => return Err(Box::new(storage_failure(error))),
    };
    source_from_row(row)
        .map_err(|message| Box::new(OperationResult::failed("MEDIA_FPS_UNSUPPORTED", message)))
}

fn validate_range(
    source: &TimelineSource,
    source_in_frame: i64,
    source_out_frame: i64,
) -> Result<(), Box<OperationResult<MainTrackClip>>> {
    if source_in_frame < 0
        || source_out_frame <= source_in_frame
        || source_out_frame > source.source_duration_frames
    {
        return Err(Box::new(OperationResult::failed(
            "MAIN_TRACK_RANGE_INVALID",
            format!(
                "源区间 [{source_in_frame}, {source_out_frame}) 超出素材 {} 的帧范围。",
                source.display_name
            ),
        )));
    }
    Ok(())
}

pub fn append_main_track_clip(
    store: &ProjectStore,
    asset_id: &str,
    source_in_frame: i64,
    source_out_frame: i64,
) -> OperationResult<MainTrackClip> {
    let source = match require_source(store, asset_id) {
        Ok(source) => source,
        Err(result) => return *result,
    };
    if let Err(result) = validate_range(&source, source_in_frame, source_out_frame) {
        return *result;
    }
    let id = Uuid::new_v4().to_string();
    match store.append_main_track_clip(&id, asset_id, source_in_frame, source_out_frame) {
        Ok(clip) => {
            let mut result = OperationResult::success(clip);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

/// 把整段素材加入主轨。源时长只在 Rust 的有理时间域内计算，前端不需要换算浮点秒。
pub fn append_full_main_track_asset(
    store: &ProjectStore,
    asset_id: &str,
) -> OperationResult<MainTrackClip> {
    let source = match require_source(store, asset_id) {
        Ok(source) => source,
        Err(result) => return *result,
    };
    append_main_track_clip(store, asset_id, 0, source.source_duration_frames)
}

pub fn insert_full_main_track_assets(
    store: &ProjectStore,
    asset_ids: &[String],
    before_id: Option<&str>,
) -> OperationResult<Vec<MainTrackClip>> {
    let mut clips = Vec::with_capacity(asset_ids.len());
    for asset_id in asset_ids {
        let source = match require_source(store, asset_id) {
            Ok(source) => source,
            Err(result) => {
                return OperationResult::failed(
                    "MEDIA_ASSET_MISSING",
                    result
                        .diagnostics
                        .first()
                        .map(|item| item.cause.clone())
                        .unwrap_or_else(|| format!("素材不存在：{asset_id}")),
                );
            }
        };
        clips.push(MainTrackClip {
            id: Uuid::new_v4().to_string(),
            source_asset_id: asset_id.clone(),
            source_in_frame: 0,
            source_out_frame: source.source_duration_frames,
            order_index: 0,
        });
    }
    match store.insert_main_track_clips(&clips, before_id) {
        Ok(inserted) => {
            let mut result = OperationResult::success(inserted);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

pub fn move_main_track_clip(
    store: &ProjectStore,
    id: &str,
    before_id: Option<&str>,
) -> OperationResult<()> {
    match store.move_main_track_clip(id, before_id) {
        Ok(()) => {
            let mut result = OperationResult::success(());
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

pub fn trim_main_track_clip(
    store: &ProjectStore,
    id: &str,
    source_in_frame: i64,
    source_out_frame: i64,
) -> OperationResult<MainTrackClip> {
    let original = match store.main_track_clip(id) {
        Ok(Some(clip)) => clip,
        Ok(None) => {
            return OperationResult::failed("MAIN_TRACK_CLIP_MISSING", format!("片段不存在：{id}"));
        }
        Err(error) => return storage_failure(error),
    };
    let source = match require_source(store, &original.source_asset_id) {
        Ok(source) => source,
        Err(result) => return *result,
    };
    if let Err(result) = validate_range(&source, source_in_frame, source_out_frame) {
        return *result;
    }
    match store.trim_main_track_clip(id, source_in_frame, source_out_frame) {
        Ok(clip) => {
            let mut result = OperationResult::success(clip);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

pub fn split_main_track_clip(
    store: &ProjectStore,
    id: &str,
    source_at_frame: i64,
) -> OperationResult<Vec<MainTrackClip>> {
    let original = match store.main_track_clip(id) {
        Ok(Some(clip)) => clip,
        Ok(None) => {
            return OperationResult::failed("MAIN_TRACK_CLIP_MISSING", format!("片段不存在：{id}"));
        }
        Err(error) => return storage_failure(error),
    };
    if source_at_frame <= original.source_in_frame || source_at_frame >= original.source_out_frame {
        return OperationResult::failed("MAIN_TRACK_SPLIT_INVALID", "拆分点必须位于片段内部。");
    }
    let right_id = Uuid::new_v4().to_string();
    match store.split_main_track_clip(id, &right_id, source_at_frame) {
        Ok((left, right)) => {
            let mut result = OperationResult::success(vec![left, right]);
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

pub fn remove_main_track_clip(store: &ProjectStore, id: &str) -> OperationResult<()> {
    match store.remove_main_track_clip(id) {
        Ok(()) => {
            let mut result = OperationResult::success(());
            result.revision = store.revision().ok();
            result
        }
        Err(error) => storage_failure(error),
    }
}

/// 编译整个项目主轨。未显式选输出帧率时，跟随主轨首个源素材。
pub fn compile_project_timeline(store: &ProjectStore, name: &str) -> OperationResult<TimelineIRv2> {
    let clips = match store.main_track_clips() {
        Ok(clips) => clips,
        Err(error) => return storage_failure(error),
    };
    let assets = match store.media_assets() {
        Ok(assets) => assets,
        Err(error) => return storage_failure(error),
    };
    let sources = match assets
        .iter()
        .cloned()
        .map(source_from_row)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(sources) => sources,
        Err(message) => return OperationResult::failed("MEDIA_FPS_UNSUPPORTED", message),
    };
    let output_rate = match store.output_rate() {
        Ok(Some(rate)) => rate,
        Ok(None) => {
            let Some(first) = clips.first() else {
                return OperationResult::failed("TIMELINE_EMPTY", "主轨没有可导出的片段。");
            };
            let Some(source) = sources
                .iter()
                .find(|source| source.asset_id == first.source_asset_id)
            else {
                return OperationResult::failed(
                    "TIMELINE_SOURCE_MISSING",
                    "主轨引用的素材不存在。",
                );
            };
            source.rate
        }
        Err(error) => return storage_failure(error),
    };
    let canvas = match store.canvas_spec() {
        Ok(canvas) => canvas,
        Err(error) => return storage_failure(error),
    };
    let assets_by_id: HashMap<&str, &MediaAssetRow> = assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset))
        .collect();
    let mut source_cuts: HashMap<&str, Vec<(i64, i64)>> = HashMap::new();
    let mut source_cut_records = Vec::<SourceCut>::new();
    let mut diagnostics = Vec::new();

    for clip in &clips {
        if source_cuts.contains_key(clip.source_asset_id.as_str()) {
            continue;
        }
        let Some(asset) = assets_by_id.get(clip.source_asset_id.as_str()) else {
            return OperationResult::failed(
                "TIMELINE_SOURCE_MISSING",
                format!("主轨引用的素材 {} 不存在。", clip.source_asset_id),
            );
        };
        let words = match store.transcript_words(&clip.source_asset_id) {
            Ok(words) => words,
            Err(error) => return storage_failure(error),
        };
        let omits = match store.active_omit_operations(&clip.source_asset_id) {
            Ok(omits) => omits,
            Err(error) => return storage_failure(error),
        };
        let cut_result = compile_rough_cut(
            asset,
            &words,
            &omits,
            &format!("{} Source Cut", asset.display_name),
            &CompileOptions::default(),
        );
        let source_is_empty = cut_result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ROUGH_CUT_EMPTY");
        let mut source_diagnostics = cut_result.diagnostics.clone();
        if source_is_empty {
            for diagnostic in &mut source_diagnostics {
                if diagnostic.code == "ROUGH_CUT_EMPTY" {
                    diagnostic.level = crate::DiagnosticLevel::Warning;
                    diagnostic.cause = "该素材的文字已全部删除，已从主轨输出中移除。".to_string();
                    diagnostic.impact = "其他主轨素材仍可继续导出。".to_string();
                    diagnostic.blocks_export = false;
                }
            }
        }
        diagnostics.extend(source_diagnostics);
        match cut_result.data {
            Some(ir) => {
                source_cut_records.extend(ir.clips.iter().map(|piece| SourceCut {
                    source_asset_id: clip.source_asset_id.clone(),
                    source_in_frame: piece.source_in_frame,
                    source_out_frame: piece.source_out_frame,
                    first_word_ordinal: piece.first_word_ordinal,
                    last_word_ordinal: piece.last_word_ordinal,
                }));
                source_cuts.insert(
                    clip.source_asset_id.as_str(),
                    ir.clips
                        .into_iter()
                        .map(|piece| (piece.source_in_frame, piece.source_out_frame))
                        .collect(),
                );
            }
            None if source_is_empty => {
                // 这个源的文字全删掉时，仅从主轨移除它的交集；其他素材仍可继续导出。
                source_cuts.insert(clip.source_asset_id.as_str(), Vec::new());
            }
            None => {
                let mut result = OperationResult::failed(
                    "SOURCE_CUT_COMPILE_FAILED",
                    format!("素材 {} 的文字删减无法编译。", asset.display_name),
                );
                result.diagnostics = diagnostics;
                return result;
            }
        }
    }

    let mut resolved_inputs = Vec::new();
    for clip in &clips {
        let Some(cuts) = source_cuts.get(clip.source_asset_id.as_str()) else {
            continue;
        };
        for (cut_index, (cut_start, cut_end)) in cuts.iter().enumerate() {
            let source_in_frame = clip.source_in_frame.max(*cut_start);
            let source_out_frame = clip.source_out_frame.min(*cut_end);
            if source_out_frame <= source_in_frame {
                continue;
            }
            resolved_inputs.push(MainTrackClip {
                id: format!("{}:cut-{cut_index}", clip.id),
                source_asset_id: clip.source_asset_id.clone(),
                source_in_frame,
                source_out_frame,
                order_index: clip.order_index,
            });
        }
    }
    let mut result = compile_main_track(name, output_rate, canvas, &sources, &resolved_inputs);
    if let Some(timeline) = result.data.as_mut() {
        timeline.source_cuts = source_cut_records;
    }
    result.diagnostics.splice(0..0, diagnostics);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::omit_words;
    use crate::storage::NewMediaAsset;

    fn insert_asset(store: &ProjectStore, id: &str, rate: FrameRate) {
        let rational = rate.rational();
        store
            .insert_media_asset(&NewMediaAsset {
                id: id.to_string(),
                kind: "video".to_string(),
                original_path: format!("/tmp/{id}.mov"),
                display_name: format!("{id}.mov"),
                duration_samples: 480_000,
                audio_sample_rate: 48_000,
                fps_num: rational.num,
                fps_den: rational.den,
                video_timebase: rate.timebase(),
                is_ntsc: rate.is_ntsc(),
                width: Some(1920),
                height: Some(1080),
                audio_channels: Some(2),
                source_tc_start_frame: Some(0),
                source_tc_is_drop_frame: false,
                ffprobe_json: "{}".to_string(),
            })
            .expect("asset inserts");
    }

    #[test]
    fn main_track_commands_compile_project_with_first_source_rate() {
        let path =
            std::env::temp_dir().join(format!("double-love-main-track-{}.sqlite", Uuid::new_v4()));
        let store = ProjectStore::open(&path).expect("store");
        insert_asset(&store, "a", FrameRate::Fps25);
        insert_asset(&store, "b", FrameRate::Fps30Ntsc);
        append_main_track_clip(&store, "a", 0, 25)
            .data
            .expect("a clip");
        append_main_track_clip(&store, "b", 0, 30)
            .data
            .expect("b clip");
        let timeline = compile_project_timeline(&store, "rough cut");
        let ir = timeline.data.expect("compiled");
        assert_eq!(ir.rate, FrameRate::Fps25);
        assert_eq!(ir.clips.len(), 2);
        assert_eq!(ir.output_duration_frames, 50);
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn appends_a_full_source_without_frontend_time_conversion() {
        let path =
            std::env::temp_dir().join(format!("double-love-full-track-{}.sqlite", Uuid::new_v4()));
        let store = ProjectStore::open(&path).expect("store");
        insert_asset(&store, "a", FrameRate::Fps25);
        let clip = append_full_main_track_asset(&store, "a")
            .data
            .expect("full clip");
        assert_eq!((clip.source_in_frame, clip.source_out_frame), (0, 250));
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn text_omits_split_a_main_track_clip_in_source_time() {
        let path =
            std::env::temp_dir().join(format!("double-love-source-cuts-{}.sqlite", Uuid::new_v4()));
        let store = ProjectStore::open(&path).expect("store");
        insert_asset(&store, "a", FrameRate::Fps25);
        store
            .insert_transcript_words(&[
                crate::storage::NewTranscriptWord {
                    word_id: "w0".to_string(),
                    asset_id: "a".to_string(),
                    ordinal: 0,
                    raw_text: "保留".to_string(),
                    display_text: "保留".to_string(),
                    language: Some("zh".to_string()),
                    start_sample: 0,
                    end_sample: 48_000,
                    confidence: Some(0.99),
                },
                crate::storage::NewTranscriptWord {
                    word_id: "w1".to_string(),
                    asset_id: "a".to_string(),
                    ordinal: 1,
                    raw_text: "删除".to_string(),
                    display_text: "删除".to_string(),
                    language: Some("zh".to_string()),
                    start_sample: 192_000,
                    end_sample: 240_000,
                    confidence: Some(0.99),
                },
            ])
            .expect("words");
        append_main_track_clip(&store, "a", 0, 250)
            .data
            .expect("track clip");
        omit_words(&store, "a", 1, 1, 0, 0).data.expect("omit");

        let timeline = compile_project_timeline(&store, "rough cut");
        let ir = timeline.data.expect("compiled");
        assert_eq!(ir.clips.len(), 2);
        assert_eq!(ir.source_cuts.len(), 2);
        assert_eq!(
            (ir.clips[0].source_in_frame, ir.clips[0].source_out_frame),
            (0, 100)
        );
        assert_eq!(
            (ir.clips[1].source_in_frame, ir.clips[1].source_out_frame),
            (125, 250)
        );
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn fully_omitted_source_does_not_block_other_main_track_sources() {
        let path =
            std::env::temp_dir().join(format!("double-love-source-omit-{}.sqlite", Uuid::new_v4()));
        let store = ProjectStore::open(&path).expect("store");
        insert_asset(&store, "a", FrameRate::Fps25);
        insert_asset(&store, "b", FrameRate::Fps25);
        store
            .insert_transcript_words(&[crate::storage::NewTranscriptWord {
                word_id: "w".to_string(),
                asset_id: "a".to_string(),
                ordinal: 0,
                raw_text: "删除".to_string(),
                display_text: "删除".to_string(),
                language: Some("zh".to_string()),
                start_sample: 0,
                end_sample: 480_000,
                confidence: Some(0.99),
            }])
            .expect("words");
        append_main_track_clip(&store, "a", 0, 250).data.expect("a");
        append_main_track_clip(&store, "b", 0, 250).data.expect("b");
        omit_words(&store, "a", 0, 0, 0, 0).data.expect("omit");
        let result = compile_project_timeline(&store, "rough cut");
        let ir = result.data.expect("b remains exportable");
        assert_eq!(ir.clips.len(), 1);
        assert_eq!(ir.clips[0].source_asset_id, "b");
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ROUGH_CUT_EMPTY" && !diagnostic.blocks_export));
        drop(store);
        std::fs::remove_file(path).ok();
    }
}
