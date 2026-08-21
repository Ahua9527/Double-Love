//! 切片契约：WordAnchor / EditOperation / TimelineIR。
//! TimelineIR schema v1 为 PRD 空白的切片自定义（验收后回写 PRD）。

use crate::rational::FrameRate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// TimelineIR 结构版本；只增不改，破坏性变更才递增。
pub const TIMELINE_IR_SCHEMA_VERSION: u32 = 1;
/// 多素材主轨的 TimelineIR。v1 保留给已生成的单素材导出物读取；新建项目一律用 v2。
pub const TIMELINE_IR_V2_SCHEMA_VERSION: u32 = 2;

/// 某个词的说话人归属。声纹与原始音频不进入该 DTO。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerAssignment {
    pub speaker_id: String,
    pub confidence: Option<f64>,
    pub evidence: String,
}

/// 项目内可见的说话人身份。自动候选与人工确认都只改这层映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerIdentity {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub color: String,
    pub confirmed: bool,
}

/// 某个源素材中由匿名/已确认说话人覆盖的声学区间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerSegment {
    pub id: String,
    pub asset_id: String,
    pub speaker_id: String,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
}

/// 跨素材合并候选，永远由用户确认后才会改变 SpeakerIdentity。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerMergeProposal {
    pub id: String,
    pub left_speaker_id: String,
    pub right_speaker_id: String,
    pub similarity: f64,
    pub evidence: String,
    // pending / accepted / rejected；自动分析只会产生 pending。
    pub status: String,
}

/// 一次本地说话人分离完成后的可展示摘要。声纹向量故意不出现在这个契约中。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerDiarizationResult {
    pub asset_id: String,
    pub segment_count: u64,
    pub speakers: Vec<SpeakerIdentity>,
    pub merge_proposals: Vec<SpeakerMergeProposal>,
}

/// 名称候选只是一条可审阅建议，绝不自动改写身份显示名。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerNameProposal {
    pub speaker_id: String,
    pub candidate_name: String,
    pub quote: String,
    pub confidence: f64,
    pub source: String,
    pub reason: String,
}

/// 用户主动请求 Agent 前展示的最小数据包。没有音频、声纹、路径或其他说话人的全文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakerNameAgentPayload {
    pub speaker_id: String,
    pub utterances: Vec<String>,
    pub instruction: String,
}

/// 词锚点：转录产物的最小剪辑单元（PRD 不变量）。
/// `start_sample`/`end_sample` 以资产音频采样率为时基的采样级整数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WordAnchor {
    pub word_id: String,
    pub asset_id: String,
    // 资产内词序，从 0 开始连续递增；UNIQUE(asset_id, ordinal)。
    pub ordinal: i64,
    pub raw_text: String,
    pub display_text: String,
    pub language: Option<String>,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
    // 合成词（纠错/对齐产物）不得驱动剪切；切片只产生 false。
    pub synthetic: bool,
    // 合成词溯源；非合成词恒为 None。
    pub source_word_ids: Option<Vec<String>>,
    #[serde(default)]
    pub speaker_assignments: Vec<SpeakerAssignment>,
}

/// 编辑操作类型。切片只实现 Omit/Restore；Move/Trim 为 PRD 契约占位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EditType {
    Omit,
    Restore,
    Move,
    Trim,
}

impl EditType {
    /// 与 serde 名一致的存储字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Omit => "omit",
            Self::Restore => "restore",
            Self::Move => "move",
            Self::Trim => "trim",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "omit" => Some(Self::Omit),
            "restore" => Some(Self::Restore),
            "move" => Some(Self::Move),
            "trim" => Some(Self::Trim),
            _ => None,
        }
    }
}

/// 编辑行为。切片只实现 RippleAv（连带音视频联动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EditBehavior {
    TextOnly,
    SubtitleOnly,
    RippleAv,
}

impl EditBehavior {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextOnly => "text_only",
            Self::SubtitleOnly => "subtitle_only",
            Self::RippleAv => "ripple_av",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "text_only" => Some(Self::TextOnly),
            "subtitle_only" => Some(Self::SubtitleOnly),
            "ripple_av" => Some(Self::RippleAv),
            _ => None,
        }
    }
}

/// 一条编辑操作：词序闭区间 [start_ordinal, end_ordinal]。
/// 删除文字 ≠ 删除底层词——omit 只是标记，restore 可完全或部分回填。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EditOperation {
    pub id: String,
    pub asset_id: String,
    pub edit_type: EditType,
    pub behavior: EditBehavior,
    pub start_ordinal: i64,
    pub end_ordinal: i64,
    // 切点前后缓冲（毫秒），转采样时按资产采样率取整。
    pub handles_before_ms: i64,
    pub handles_after_ms: i64,
    // 被更新的操作回填 supersede 链；活跃操作为 None。
    pub superseded_by: Option<String>,
    pub revision: i64,
    pub created_at: String,
}

/// 文本视图分段（TranscriptView 的渲染单元）。
/// 由词序列纯函数生成（segment.rs），不落表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranscriptSegment {
    pub index: i64,
    pub start_ordinal: i64,
    pub end_ordinal: i64,
    pub text: String,
    pub start_sample: i64,
    pub end_sample: i64,
    // 整段被活跃 omit 覆盖（UI 划线）。
    pub omitted: bool,
    pub partially_omitted: bool,
}

/// TranscriptView 一次渲染所需的全部数据（segment.rs::transcript_view 装配）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranscriptViewData {
    pub words: Vec<WordAnchor>,
    pub segments: Vec<TranscriptSegment>,
    // 当前活跃的 omit 操作（恢复操作的入口）。
    pub omits: Vec<EditOperation>,
}

/// TimelineIR 中的一个片段：源素材的连续帧区间 → 输出时间线的连续帧区间。
/// 帧区间均为 [in, out) 半开。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IrClip {
    pub clip_index: i64,
    pub source_in_frame: i64,
    pub source_out_frame: i64,
    pub timeline_start_frame: i64,
    pub timeline_end_frame: i64,
    pub first_word_ordinal: i64,
    pub last_word_ordinal: i64,
}

/// Output Time Map 的一段：源采样区间 ↔ 输出帧区间的线性映射。
/// SRT/XML/FCPXML/MD 未来只能从这同一份 Map 生成（PRD 不变量）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapSegment {
    pub src_start_sample: i64,
    pub src_end_sample: i64,
    pub out_start_frame: i64,
    pub out_end_frame: i64,
}

/// 时间线中间表示：编译器产物、导出器唯一输入。
/// Exporter 禁止重算剪辑时间，只做 IR → 目标格式映射（PRD 不变量）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimelineIR {
    pub schema_version: u32,
    pub name: String,
    pub rate: FrameRate,
    pub source_asset_id: String,
    pub source_duration_frames: i64,
    pub output_duration_frames: i64,
    pub clips: Vec<IrClip>,
    pub output_map: Vec<MapSegment>,
}

/// 画布适配方式。画布参数是项目级设置，不产生每片段隐式变换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CanvasFit {
    Contain,
    Cover,
}

/// 项目级画布。时间仍使用帧/采样整数；视觉参数只用于预览与渲染。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CanvasSpec {
    pub width: i64,
    pub height: i64,
    pub background: String,
    pub fit: CanvasFit,
    pub position_x: f64,
    pub position_y: f64,
    pub scale: f64,
    pub rotation_degrees: f64,
    pub opacity: f64,
}

impl Default for CanvasSpec {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            background: "#000000".to_string(),
            fit: CanvasFit::Contain,
            position_x: 0.0,
            position_y: 0.0,
            scale: 1.0,
            rotation_degrees: 0.0,
            opacity: 1.0,
        }
    }
}

/// 多素材时间线中的源媒体事实。所有路径均为只读引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimelineSource {
    pub asset_id: String,
    pub display_name: String,
    pub original_path: String,
    pub rate: FrameRate,
    pub source_duration_frames: i64,
    pub audio_sample_rate: i64,
    pub audio_channels: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source_tc_start_frame: Option<i64>,
    pub source_tc_is_drop_frame: bool,
}

/// 源素材时间域内由文字删减得到的可恢复保留区间。它不改变原始媒体，只作为主轨
/// 编译前的 SourceCut 层；用户的 omit/restore 记录仍保存在项目历史中。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceCut {
    pub source_asset_id: String,
    pub source_in_frame: i64,
    pub source_out_frame: i64,
    pub first_word_ordinal: i64,
    pub last_word_ordinal: i64,
}

/// 主轨的一条用户编辑记录，`source_*` 永远处于该源自身的帧率域。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MainTrackClip {
    pub id: String,
    pub source_asset_id: String,
    pub source_in_frame: i64,
    pub source_out_frame: i64,
    pub order_index: i64,
}

/// v2 编译后的主轨片段。输出帧一律属于项目输出帧率域。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResolvedTimelineClip {
    pub id: String,
    pub source_asset_id: String,
    pub source_in_frame: i64,
    pub source_out_frame: i64,
    pub timeline_start_frame: i64,
    pub timeline_end_frame: i64,
}

/// 多源 Output Time Map。字幕、播放和 NLE 导出必须消费同一组段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputMapSegment {
    pub source_asset_id: String,
    pub clip_id: String,
    pub src_start_sample: i64,
    pub src_end_sample: i64,
    pub out_start_frame: i64,
    pub out_end_frame: i64,
}

/// 多素材主轨的唯一输出中间表示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimelineIRv2 {
    pub schema_version: u32,
    pub name: String,
    pub rate: FrameRate,
    pub canvas: CanvasSpec,
    pub sources: Vec<TimelineSource>,
    pub source_cuts: Vec<SourceCut>,
    pub clips: Vec<ResolvedTimelineClip>,
    pub output_duration_frames: i64,
    pub output_map: Vec<OutputMapSegment>,
}

/// 项目级字幕外观；单 Cue 不覆盖该对象，避免首版复杂度失控。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SubtitleStyle {
    pub font_family: String,
    pub font_weight: i64,
    pub font_size: f64,
    pub text_color: String,
    pub outline_color: String,
    pub outline_width: f64,
    pub shadow_color: String,
    pub shadow_offset_x: f64,
    pub shadow_offset_y: f64,
    pub shadow_blur: f64,
    pub background_color: String,
    pub background_radius: f64,
    pub background_padding_x: f64,
    pub background_padding_y: f64,
    pub position_x: f64,
    pub position_y: f64,
    pub max_width_ratio: f64,
    pub max_lines: i64,
    pub target_characters_per_line: i64,
    pub show_speaker: bool,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_family: "PingFang SC".to_string(),
            font_weight: 600,
            font_size: 52.0,
            text_color: "#FFFFFF".to_string(),
            outline_color: "#000000".to_string(),
            outline_width: 3.0,
            shadow_color: "#00000099".to_string(),
            shadow_offset_x: 0.0,
            shadow_offset_y: 3.0,
            shadow_blur: 6.0,
            background_color: "#00000000".to_string(),
            background_radius: 12.0,
            background_padding_x: 16.0,
            background_padding_y: 8.0,
            position_x: 0.5,
            position_y: 0.88,
            max_width_ratio: 0.82,
            max_lines: 2,
            target_characters_per_line: 22,
            show_speaker: false,
        }
    }
}

/// 输出时间域内的一条字幕 Cue。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SubtitleCue {
    pub id: String,
    pub source_word_ids: Vec<String>,
    pub speaker_id: Option<String>,
    // 项目级身份映射后的显示名；匿名或待确认时为 None。
    pub speaker_name: Option<String>,
    pub start_frame: i64,
    pub end_frame: i64,
    pub text: String,
}

/// 某个导出目标对本次时间线的明确保留与有损边界；不把“可以导入”伪装成“视觉完全一致”。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompatibilityReport {
    pub target: String,
    pub preserved: Vec<String>,
    pub limitations: Vec<String>,
}

/// 所有项目级导出共享的只读预览。Apply 只把它写成目标格式，禁止重新编译时间线。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectExportPreview {
    pub timeline: TimelineIRv2,
    pub subtitle_cues: Vec<SubtitleCue>,
    pub compatibility: Vec<CompatibilityReport>,
}

/// 可审阅的项目历史条目。历史恢复本身也会生成一条新的 revision。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RevisionHistoryEntry {
    pub revision: u64,
    pub operation: String,
    pub committed_at: String,
    pub restorable: bool,
}

/// 媒体资产状态机：imported → prepared → transcribed。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    Imported,
    Prepared,
    Transcribed,
}

impl AssetStatus {
    /// 与 SQLite 中保存的状态值保持一致。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Prepared => "prepared",
            Self::Transcribed => "transcribed",
        }
    }

    /// 未知状态不被悄悄降级，以免 UI 把已完成的转录误显示成“待转录”。
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "imported" => Some(Self::Imported),
            "prepared" => Some(Self::Prepared),
            "transcribed" => Some(Self::Transcribed),
            _ => None,
        }
    }
}

/// 导入后的资产摘要（import_media 命令返回值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaAssetSummary {
    pub id: String,
    pub display_name: String,
    // 时长（以 `audio_sample_rate` 为时基的采样数）。
    pub duration_samples: i64,
    pub audio_sample_rate: i64,
    pub rate: FrameRate,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub audio_channels: Option<i64>,
    pub status: AssetStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_ir_serializes_with_schema_version() {
        let ir = TimelineIR {
            schema_version: TIMELINE_IR_SCHEMA_VERSION,
            name: "ROUGH CUT".to_string(),
            rate: FrameRate::Fps25,
            source_asset_id: "asset-1".to_string(),
            source_duration_frames: 2500,
            output_duration_frames: 2000,
            clips: vec![IrClip {
                clip_index: 0,
                source_in_frame: 0,
                source_out_frame: 2000,
                timeline_start_frame: 0,
                timeline_end_frame: 2000,
                first_word_ordinal: 0,
                last_word_ordinal: 99,
            }],
            output_map: vec![MapSegment {
                src_start_sample: 0,
                src_end_sample: 3_840_000,
                out_start_frame: 0,
                out_end_frame: 2000,
            }],
        };
        let json = serde_json::to_value(&ir).expect("serialize");
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["rate"], "fps_25");
        assert_eq!(json["clips"][0]["source_out_frame"], 2000);
    }

    #[test]
    fn edit_operation_uses_snake_case_enums() {
        let op = EditOperation {
            id: "op-1".to_string(),
            asset_id: "asset-1".to_string(),
            edit_type: EditType::Omit,
            behavior: EditBehavior::RippleAv,
            start_ordinal: 10,
            end_ordinal: 25,
            handles_before_ms: 120,
            handles_after_ms: 120,
            superseded_by: None,
            revision: 1,
            created_at: "2026-08-16T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&op).expect("serialize");
        assert_eq!(json["edit_type"], "omit");
        assert_eq!(json["behavior"], "ripple_av");
    }
}
