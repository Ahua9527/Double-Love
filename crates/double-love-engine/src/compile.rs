//! 粗剪编译器：词 + 活跃 omit → TimelineIR + 单一 Output Time Map（纯函数）。
//! 次序：omit 词序区间 → handles 外扩（ms→采样）→ clamp → 排序合并 →
//! kept 补集 → 帧量化向内收缩（in=ceil/out=floor，宁少勿多）→
//! 过碎段丢弃（<2 帧）→ 小空洞并段（<merge_gap）→ ripple 从 0 累排。
//! f64 不进任何一步；采样↔帧换算走 rational.rs 的整数运算。

use crate::contracts::{
    EditOperation, IrClip, MapSegment, TIMELINE_IR_SCHEMA_VERSION, TimelineIR, WordAnchor,
};
use crate::rational::{FrameRate, Rational, Round, frame_to_samples, samples_to_frame};
use crate::storage::MediaAssetRow;
use crate::{Diagnostic, DiagnosticLevel, OperationResult};

/// 编译选项。
pub struct CompileOptions {
    /// 小于该帧数的段间空洞被并段（消除听不出的小切）
    pub merge_gap_frames: i64,
    /// 预留：切点吸附静音（第二刀）
    pub snap_to_silence: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            merge_gap_frames: 4,
            snap_to_silence: false,
        }
    }
}

fn warning(code: &str, cause: String) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: code.to_string(),
        cause,
        object_id: None,
        impact: "该段不进入导出".to_string(),
        blocks_export: false,
        suggested_action: None,
    }
}

/// 编译粗剪。words 按 ordinal 升序；omits 只需活跃 omit（superseded 的不传）。
pub fn compile_rough_cut(
    asset: &MediaAssetRow,
    words: &[WordAnchor],
    omits: &[EditOperation],
    name: &str,
    options: &CompileOptions,
) -> OperationResult<TimelineIR> {
    let rate = match FrameRate::from_rational(&Rational::new(asset.fps_num, asset.fps_den)) {
        Some(rate) => rate,
        None => {
            return OperationResult::failed(
                "STORAGE_CORRUPT",
                format!("资产帧率无法解析：{}/{}", asset.fps_num, asset.fps_den),
            );
        }
    };
    let sample_rate = asset.audio_sample_rate;
    let duration = asset.duration_samples;

    if words.iter().any(|word| word.synthetic) {
        return OperationResult::failed(
            "ROUGH_CUT_SYNTHETIC_WORDS",
            "合成词不得驱动剪切（本切片不应产生合成词）",
        );
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // 所有词都被删除 → 直接判空（头尾静默不留，避免导出纯静默时间线）
    if !words.is_empty() {
        let all_omitted = words.iter().all(|word| {
            omits
                .iter()
                .any(|op| word.ordinal >= op.start_ordinal && word.ordinal <= op.end_ordinal)
        });
        if all_omitted && !omits.is_empty() {
            let mut result = OperationResult::failed(
                "ROUGH_CUT_EMPTY",
                "所有文字都被删除，粗剪为空。".to_string(),
            );
            result.diagnostics[0].blocks_export = true;
            result.diagnostics[0].suggested_action = Some("恢复至少一段文字后再导出。".to_string());
            return result;
        }
    }

    // 1) omit 词序区间 → 采样区间（handles 外扩 + clamp）
    let ms_to_samples = |ms: i64| ms * sample_rate / 1_000;
    let mut omitted: Vec<(i64, i64)> = Vec::new();
    for op in omits {
        if words.is_empty() {
            diagnostics.push(warning(
                "ROUGH_CUT_STALE_EDIT",
                "编辑操作存在但没有任何转录词，已忽略。".to_string(),
            ));
            break;
        }
        let last_ordinal = words.len() as i64 - 1;
        if op.start_ordinal > last_ordinal {
            diagnostics.push(warning(
                "ROUGH_CUT_STALE_EDIT",
                format!(
                    "删除操作 [{}, {}] 超出当前词数 {}（可能重新转录过），已忽略。",
                    op.start_ordinal,
                    op.end_ordinal,
                    words.len()
                ),
            ));
            continue;
        }
        let start = op.start_ordinal;
        let end = op.end_ordinal.min(last_ordinal);
        let start_sample =
            (words[start as usize].start_sample - ms_to_samples(op.handles_before_ms)).max(0);
        let end_sample =
            (words[end as usize].end_sample + ms_to_samples(op.handles_after_ms)).min(duration);
        if end_sample > start_sample {
            omitted.push((start_sample, end_sample));
        }
    }

    // 2) 排序合并 omit 区间
    omitted.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::with_capacity(omitted.len());
    for (start, end) in omitted {
        match merged.last_mut() {
            Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
            _ => merged.push((start, end)),
        }
    }

    // 3) kept 补集（全素材范围内）
    let mut kept: Vec<(i64, i64)> = Vec::with_capacity(merged.len() + 1);
    let mut cursor = 0_i64;
    for (start, end) in &merged {
        if *start > cursor {
            kept.push((cursor, *start));
        }
        cursor = cursor.max(*end);
    }
    if cursor < duration {
        kept.push((cursor, duration));
    }

    // 4) 帧量化向内收缩；<2 帧的碎段丢弃
    let total_frames = samples_to_frame(duration, rate, sample_rate, Round::Floor);
    let mut clips: Vec<(i64, i64)> = Vec::with_capacity(kept.len()); // (in_frame, out_frame)
    for (start_sample, end_sample) in kept {
        let in_frame = samples_to_frame(start_sample, rate, sample_rate, Round::Ceil);
        let out_frame = samples_to_frame(end_sample, rate, sample_rate, Round::Floor);
        if out_frame - in_frame < 2 {
            if end_sample > start_sample {
                diagnostics.push(warning(
                    "ROUGH_CUT_CLIP_DROPPED",
                    format!(
                        "量化后不足 2 帧的碎段已丢弃（采样 [{start_sample}, {end_sample}）→ 帧 [{in_frame}, {out_frame}）"
                    ),
                ));
            }
            continue;
        }
        clips.push((in_frame, out_frame));
    }

    // 5) 小空洞并段（相邻段间隔 < merge_gap_frames）
    let mut merged_clips: Vec<(i64, i64)> = Vec::with_capacity(clips.len());
    for (in_frame, out_frame) in clips {
        match merged_clips.last_mut() {
            Some((_, last_out)) if in_frame - *last_out < options.merge_gap_frames => {
                *last_out = out_frame
            }
            _ => merged_clips.push((in_frame, out_frame)),
        }
    }

    if merged_clips.is_empty() {
        let mut result = OperationResult::failed(
            "ROUGH_CUT_EMPTY",
            "删除后没有任何可导出的片段。".to_string(),
        );
        result.diagnostics[0].blocks_export = true;
        result.diagnostics[0].suggested_action =
            Some("恢复至少一段文字，或减小删除范围。".to_string());
        return result;
    }

    // 6) ripple 从 0 累排 + 词归属 + Output Time Map
    let mut ir_clips = Vec::with_capacity(merged_clips.len());
    let mut map = Vec::with_capacity(merged_clips.len());
    let mut timeline_cursor = 0_i64;
    for (index, (in_frame, out_frame)) in merged_clips.iter().enumerate() {
        let length = out_frame - in_frame;
        let timeline_start = timeline_cursor;
        let timeline_end = timeline_start + length;
        let src_start_sample = frame_to_samples(*in_frame, rate, sample_rate);
        let src_end_sample = frame_to_samples(*out_frame, rate, sample_rate);

        // 词归属：与片段采样区间相交的词（无相交词记 -1）
        let mut first_word = -1_i64;
        let mut last_word = -1_i64;
        for word in words {
            if word.end_sample > src_start_sample && word.start_sample < src_end_sample {
                if first_word < 0 {
                    first_word = word.ordinal;
                }
                last_word = word.ordinal;
            }
        }

        ir_clips.push(IrClip {
            clip_index: index as i64,
            source_in_frame: *in_frame,
            source_out_frame: *out_frame,
            timeline_start_frame: timeline_start,
            timeline_end_frame: timeline_end,
            first_word_ordinal: first_word,
            last_word_ordinal: last_word,
        });
        map.push(MapSegment {
            src_start_sample,
            src_end_sample,
            out_start_frame: timeline_start,
            out_end_frame: timeline_end,
        });
        timeline_cursor = timeline_end;
    }

    let ir = TimelineIR {
        schema_version: TIMELINE_IR_SCHEMA_VERSION,
        name: name.to_string(),
        rate,
        source_asset_id: asset.id.clone(),
        source_duration_frames: total_frames,
        output_duration_frames: timeline_cursor,
        clips: ir_clips,
        output_map: map,
    };
    let mut result = OperationResult::success(ir);
    result.counts.total = words.len() as u64;
    result.counts.processed = result
        .data
        .as_ref()
        .map(|ir| ir.clips.len() as u64)
        .unwrap_or(0);
    result.diagnostics = diagnostics;
    result
}

/// 源采样 → 输出帧（段内线性映射，floor；区间外 None）。
/// 这是 SRT/未来 FCPXML 共用的唯一换算入口（单一 Output Time Map）。
pub fn source_to_output(map: &[MapSegment], sample: i64) -> Option<i64> {
    let segment = map
        .iter()
        .find(|s| sample >= s.src_start_sample && sample < s.src_end_sample)?;
    let src_len = (segment.src_end_sample - segment.src_start_sample) as i128;
    let out_len = (segment.out_end_frame - segment.out_start_frame) as i128;
    let offset = (sample - segment.src_start_sample) as i128;
    Some(segment.out_start_frame + (offset * out_len / src_len) as i64)
}

/// 输出帧 → 源采样（段内线性映射，floor；区间外 None）。
pub fn output_to_source(map: &[MapSegment], frame: i64) -> Option<i64> {
    let segment = map
        .iter()
        .find(|s| frame >= s.out_start_frame && frame < s.out_end_frame)?;
    let out_len = (segment.out_end_frame - segment.out_start_frame) as i128;
    let src_len = (segment.src_end_sample - segment.src_start_sample) as i128;
    let offset = (frame - segment.out_start_frame) as i128;
    Some(segment.src_start_sample + (offset * src_len / out_len) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::EditType;

    /// 25fps / 48kHz / 10 秒资产（每帧 1920 采样，共 250 帧）。
    fn asset() -> MediaAssetRow {
        MediaAssetRow {
            id: "a1".to_string(),
            kind: "video".to_string(),
            original_path: "/tmp/synthetic.mp4".to_string(),
            display_name: "synthetic".to_string(),
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
            source_tc_is_drop_frame: false,
            prepared_wav_path: Some("/tmp/prepared.wav".to_string()),
            status: "transcribed".to_string(),
        }
    }

    fn word(ordinal: i64, start_sample: i64, end_sample: i64) -> WordAnchor {
        WordAnchor {
            word_id: format!("w{ordinal}"),
            asset_id: "a1".to_string(),
            ordinal,
            raw_text: format!("词{ordinal}"),
            display_text: format!("词{ordinal}"),
            language: Some("zh".to_string()),
            start_sample,
            end_sample,
            confidence: Some(0.99),
            synthetic: false,
            source_word_ids: None,
            speaker_assignments: Vec::new(),
        }
    }

    fn omit(start: i64, end: i64, handles_ms: i64) -> EditOperation {
        EditOperation {
            id: format!("op-{start}-{end}"),
            asset_id: "a1".to_string(),
            edit_type: EditType::Omit,
            behavior: crate::contracts::EditBehavior::RippleAv,
            start_ordinal: start,
            end_ordinal: end,
            handles_before_ms: handles_ms,
            handles_after_ms: handles_ms,
            superseded_by: None,
            revision: 1,
            created_at: "2026-08-17T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn no_omits_yields_single_full_clip() {
        let words = vec![word(0, 48_000, 96_000), word(1, 100_000, 140_000)];
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        let ir = result.data.expect("ir");
        assert_eq!(ir.clips.len(), 1);
        assert_eq!(ir.clips[0].source_in_frame, 0);
        assert_eq!(ir.clips[0].source_out_frame, 250);
        assert_eq!(ir.clips[0].timeline_start_frame, 0);
        assert_eq!(ir.clips[0].timeline_end_frame, 250);
        assert_eq!(ir.output_duration_frames, 250);
        assert_eq!(ir.clips[0].first_word_ordinal, 0);
        assert_eq!(ir.clips[0].last_word_ordinal, 1);
        assert_eq!(ir.output_map.len(), 1);
    }

    #[test]
    fn omit_middle_splits_with_handles_and_inward_quantize() {
        // 词1 [48000, 96000]，handles 120ms=5760 采样 → omit [42240, 101760]
        let words = vec![
            word(0, 0, 40_000),
            word(1, 48_000, 96_000),
            word(2, 120_000, 200_000),
        ];
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[omit(1, 1, 120)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        let ir = result.data.expect("ir");
        assert_eq!(ir.clips.len(), 2);
        // kept1 [0, 42240)：42240/1920 = 22.0 → [0, 22)
        assert_eq!(
            (ir.clips[0].source_in_frame, ir.clips[0].source_out_frame),
            (0, 22)
        );
        // kept2 [101760, 480000)：101760/1920 = 53.0 → [53, 250)
        assert_eq!(
            (ir.clips[1].source_in_frame, ir.clips[1].source_out_frame),
            (53, 250)
        );
        // ripple：clip1 [0,22) → clip2 从 22 起
        assert_eq!(ir.clips[1].timeline_start_frame, 22);
        assert_eq!(ir.output_duration_frames, 22 + (250 - 53));
        // map 段数与 clips 一致，帧↔采样边界精确互逆
        assert_eq!(ir.output_map.len(), 2);
        assert_eq!(ir.output_map[1].src_start_sample, 53 * 1920);
    }

    #[test]
    fn small_gaps_between_clips_are_merged() {
        // 两个 omit 只隔 3 帧：量化后 kept 岛 <4 帧 → 全部并成一段
        // 词1 [frame 20..22]=[38400, 42240+...] 用 0 handles 精确落帧
        let words = vec![
            word(0, 0, 30_000),
            word(1, 38_400, 42_240), // frame [20, 22)
            word(2, 43_000, 47_000),
            word(3, 48_000, 51_840), // frame [25, 27)
            word(4, 60_000, 90_000),
        ];
        let zero = |s, e| omit(s, e, 0);
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[zero(1, 1), zero(3, 3)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        let ir = result.data.expect("ir");
        // kept: [0,38400)→[0,20); [42240,48000)→[22,25) 3 帧; [51840,480000)→[27,250)
        // [0,20) 与 [22,25) 间隔 2 帧 <4 → 并 [0,25)；再与 [27,250) 间隔 2 帧 <4 → 并 [0,250)
        assert_eq!(ir.clips.len(), 1);
        assert_eq!(
            (ir.clips[0].source_in_frame, ir.clips[0].source_out_frame),
            (0, 250)
        );
    }

    #[test]
    fn tiny_quantized_clip_is_dropped_with_diagnostic() {
        // omit 词1 [460800, 472320]，after handle 120ms → kept 尾段 [478080, 480000) = 1 帧 → 丢弃
        let words = vec![word(0, 0, 400_000), word(1, 460_800, 472_320)];
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[omit(1, 1, 120)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        let ir = result.data.expect("ir");
        assert_eq!(ir.clips.len(), 1);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "ROUGH_CUT_CLIP_DROPPED" && !d.blocks_export)
        );
    }

    #[test]
    fn omitting_every_word_fails_as_empty() {
        let words = vec![word(0, 48_000, 96_000), word(1, 100_000, 140_000)];
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[omit(0, 1, 120)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        assert_eq!(result.status, crate::OperationStatus::Failed);
        assert_eq!(result.diagnostics[0].code, "ROUGH_CUT_EMPTY");
        assert!(result.diagnostics[0].blocks_export);
    }

    #[test]
    fn output_map_round_trips_at_segment_boundaries() {
        let words = vec![
            word(0, 0, 40_000),
            word(1, 48_000, 96_000),
            word(2, 120_000, 200_000),
        ];
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[omit(1, 1, 120)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        let ir = result.data.expect("ir");
        for segment in &ir.output_map {
            // 段首采样 → 输出帧 → 回到源采样：边界精确互逆
            let out =
                source_to_output(&ir.output_map, segment.src_start_sample).expect("start maps");
            assert_eq!(out, segment.out_start_frame);
            let back = output_to_source(&ir.output_map, out).expect("frame maps back");
            assert_eq!(back, segment.src_start_sample);
            // 段尾是开区间端点，不得映射
            assert!(source_to_output(&ir.output_map, segment.src_end_sample).is_none());
        }
        assert_eq!(
            output_to_source(&ir.output_map, ir.output_duration_frames),
            None,
            "输出末端开区间外"
        );
    }

    #[test]
    fn stale_edit_after_retranscribe_is_ignored_with_warning() {
        let words = vec![word(0, 0, 96_000)]; // 重新转录后只剩 1 词
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[omit(5, 9, 120)],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        assert_eq!(result.status, crate::OperationStatus::Success);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "ROUGH_CUT_STALE_EDIT")
        );
    }

    #[test]
    fn synthetic_words_are_rejected() {
        let mut words = vec![word(0, 0, 96_000)];
        words[0].synthetic = true;
        let result = compile_rough_cut(
            &asset(),
            &words,
            &[],
            "ROUGH CUT",
            &CompileOptions::default(),
        );
        assert_eq!(result.diagnostics[0].code, "ROUGH_CUT_SYNTHETIC_WORDS");
    }
}
