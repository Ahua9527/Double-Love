//! 多素材主轨编译器。源素材各自保留原帧率和时间码，只有输出主轨进入项目帧率域。

use std::collections::HashMap;

use crate::contracts::{
    CanvasSpec, MainTrackClip, OutputMapSegment, ResolvedTimelineClip,
    TIMELINE_IR_V2_SCHEMA_VERSION, TimelineIRv2, TimelineSource,
};
use crate::rational::{FrameRate, Round, convert_frame_rate, frame_to_samples};
use crate::{Diagnostic, DiagnosticLevel, OperationResult};

fn error(code: &str, cause: impl Into<String>) -> OperationResult<TimelineIRv2> {
    let mut result = OperationResult::failed(code, cause.into());
    result.diagnostics[0].object_id = None;
    result
}

fn warning(code: &str, cause: impl Into<String>, object_id: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: code.to_string(),
        cause: cause.into(),
        object_id: Some(object_id.to_string()),
        impact: "该片段按输出帧率量化。".to_string(),
        blocks_export: false,
        suggested_action: None,
    }
}

/// 编译用户排序后的主轨。每一个输入 Clip 都是源媒体上的连续区间；源内的文字 omit
/// 已经由调用方拆成多个 MainTrackClip，因此这个函数只做混合帧率与 ripple 排列。
pub fn compile_main_track(
    name: &str,
    output_rate: FrameRate,
    canvas: CanvasSpec,
    sources: &[TimelineSource],
    clips: &[MainTrackClip],
) -> OperationResult<TimelineIRv2> {
    if clips.is_empty() {
        return error("TIMELINE_EMPTY", "主轨没有可导出的片段。");
    }

    let source_by_id: HashMap<&str, &TimelineSource> = sources
        .iter()
        .map(|source| (source.asset_id.as_str(), source))
        .collect();
    let mut ordered = clips.to_vec();
    ordered.sort_by_key(|clip| clip.order_index);
    let requested_clip_count = ordered.len() as u64;

    let mut diagnostics = Vec::new();
    let mut resolved = Vec::with_capacity(ordered.len());
    let mut output_map = Vec::with_capacity(ordered.len());
    let mut cursor = 0_i64;

    for clip in ordered {
        let Some(source) = source_by_id.get(clip.source_asset_id.as_str()) else {
            return error(
                "TIMELINE_SOURCE_MISSING",
                format!(
                    "主轨片段 {} 引用了不存在的素材 {}。",
                    clip.id, clip.source_asset_id
                ),
            );
        };
        if clip.source_in_frame < 0
            || clip.source_out_frame <= clip.source_in_frame
            || clip.source_out_frame > source.source_duration_frames
        {
            return error(
                "TIMELINE_CLIP_RANGE_INVALID",
                format!("主轨片段 {} 的源入出点超出素材范围。", clip.id),
            );
        }

        let source_length = clip.source_out_frame - clip.source_in_frame;
        // 对时长向下量化，保证不把源素材边界扩展到用户未选择的帧。
        let output_length =
            convert_frame_rate(source_length, source.rate, output_rate, Round::Floor);
        if output_length <= 0 {
            diagnostics.push(warning(
                "TIMELINE_CLIP_DROPPED",
                "跨帧率量化后不足一帧，已跳过。",
                &clip.id,
            ));
            continue;
        }
        let timeline_start_frame = cursor;
        let timeline_end_frame = cursor + output_length;
        let src_start_sample =
            frame_to_samples(clip.source_in_frame, source.rate, source.audio_sample_rate);
        let src_end_sample =
            frame_to_samples(clip.source_out_frame, source.rate, source.audio_sample_rate);

        resolved.push(ResolvedTimelineClip {
            id: clip.id.clone(),
            source_asset_id: clip.source_asset_id.clone(),
            source_in_frame: clip.source_in_frame,
            source_out_frame: clip.source_out_frame,
            timeline_start_frame,
            timeline_end_frame,
        });
        output_map.push(OutputMapSegment {
            source_asset_id: clip.source_asset_id,
            clip_id: clip.id,
            src_start_sample,
            src_end_sample,
            out_start_frame: timeline_start_frame,
            out_end_frame: timeline_end_frame,
        });
        cursor = timeline_end_frame;
    }

    if resolved.is_empty() {
        return error("TIMELINE_EMPTY", "所有主轨片段在输出帧率量化后均为空。");
    }

    let mut result = OperationResult::success(TimelineIRv2 {
        schema_version: TIMELINE_IR_V2_SCHEMA_VERSION,
        name: name.to_string(),
        rate: output_rate,
        canvas,
        sources: sources.to_vec(),
        source_cuts: Vec::new(),
        clips: resolved,
        output_duration_frames: cursor,
        output_map,
    });
    result.counts.total = requested_clip_count;
    result.counts.processed = result.data.as_ref().map_or(0, |ir| ir.clips.len() as u64);
    result.diagnostics = diagnostics;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, rate: FrameRate, duration: i64) -> TimelineSource {
        TimelineSource {
            asset_id: id.to_string(),
            display_name: format!("{id}.mov"),
            original_path: format!("/tmp/{id}.mov"),
            rate,
            source_duration_frames: duration,
            audio_sample_rate: 48_000,
            audio_channels: Some(2),
            width: Some(1920),
            height: Some(1080),
            source_tc_start_frame: Some(0),
            source_tc_is_drop_frame: false,
        }
    }

    #[test]
    fn compiles_mixed_rate_sources_into_one_output_clock() {
        let result = compile_main_track(
            "Interview rough cut",
            FrameRate::Fps25,
            CanvasSpec::default(),
            &[
                source("a", FrameRate::Fps25, 250),
                source("b", FrameRate::Fps30Ntsc, 300),
            ],
            &[
                MainTrackClip {
                    id: "clip-a".to_string(),
                    source_asset_id: "a".to_string(),
                    source_in_frame: 0,
                    source_out_frame: 50,
                    order_index: 1,
                },
                MainTrackClip {
                    id: "clip-b".to_string(),
                    source_asset_id: "b".to_string(),
                    source_in_frame: 0,
                    source_out_frame: 60,
                    order_index: 2,
                },
            ],
        );
        let ir = result.data.expect("timeline");
        assert_eq!(ir.schema_version, 2);
        assert_eq!(ir.clips.len(), 2);
        assert_eq!(ir.clips[0].timeline_end_frame, 50);
        // 60 frames @ 29.97fps = 2.002s, floor 到 25fps 为 50 帧。
        assert_eq!(ir.clips[1].timeline_start_frame, 50);
        assert_eq!(ir.clips[1].timeline_end_frame, 100);
        assert_eq!(ir.output_duration_frames, 100);
        assert_eq!(ir.output_map[1].source_asset_id, "b");
    }

    #[test]
    fn rejects_unknown_source_and_invalid_range() {
        let no_source = compile_main_track(
            "bad",
            FrameRate::Fps25,
            CanvasSpec::default(),
            &[],
            &[MainTrackClip {
                id: "missing".to_string(),
                source_asset_id: "nope".to_string(),
                source_in_frame: 0,
                source_out_frame: 1,
                order_index: 0,
            }],
        );
        assert_eq!(no_source.diagnostics[0].code, "TIMELINE_SOURCE_MISSING");

        let invalid_range = compile_main_track(
            "bad",
            FrameRate::Fps25,
            CanvasSpec::default(),
            &[source("a", FrameRate::Fps25, 10)],
            &[MainTrackClip {
                id: "range".to_string(),
                source_asset_id: "a".to_string(),
                source_in_frame: 0,
                source_out_frame: 11,
                order_index: 0,
            }],
        );
        assert_eq!(
            invalid_range.diagnostics[0].code,
            "TIMELINE_CLIP_RANGE_INVALID"
        );
    }

    #[test]
    fn supports_the_full_mixed_rate_beta_matrix_without_float_time() {
        let rates = [
            FrameRate::Fps24Ntsc,
            FrameRate::Fps24,
            FrameRate::Fps25,
            FrameRate::Fps30Ntsc,
            FrameRate::Fps30,
            FrameRate::Fps50,
            FrameRate::Fps60Ntsc,
            FrameRate::Fps60,
            FrameRate::Fps120Ntsc,
            FrameRate::Fps120,
        ];
        let sources = rates
            .iter()
            .enumerate()
            .map(|(index, rate)| source(&format!("source-{index}"), *rate, rate.timebase() * 4))
            .collect::<Vec<_>>();
        let clips = sources
            .iter()
            .enumerate()
            .map(|(index, source)| MainTrackClip {
                id: format!("clip-{index}"),
                source_asset_id: source.asset_id.clone(),
                source_in_frame: 0,
                source_out_frame: source.rate.timebase(),
                order_index: index as i64,
            })
            .collect::<Vec<_>>();
        let result = compile_main_track(
            "matrix",
            FrameRate::Fps25,
            CanvasSpec::default(),
            &sources,
            &clips,
        );
        assert_eq!(result.status, crate::OperationStatus::Success);
        let ir = result.data.expect("timeline");
        assert_eq!(ir.clips.len(), rates.len());
        assert!(ir.output_duration_frames > 0);
    }
}
