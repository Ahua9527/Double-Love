//! 切片契约：WordAnchor / EditOperation / TimelineIR。
//! TimelineIR schema v1 为 PRD 空白的切片自定义（验收后回写 PRD）。

use crate::rational::FrameRate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// TimelineIR 结构版本；只增不改，破坏性变更才递增。
pub const TIMELINE_IR_SCHEMA_VERSION: u32 = 1;

/// 词锚点：转录产物的最小剪辑单元（PRD 不变量）。
/// `start_sample`/`end_sample` 以资产音频采样率为时基的采样级整数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WordAnchor {
    pub word_id: String,
    pub asset_id: String,
    /// 资产内词序，从 0 开始连续递增；UNIQUE(asset_id, ordinal)。
    pub ordinal: i64,
    pub raw_text: String,
    pub display_text: String,
    pub language: Option<String>,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
    /// 合成词（纠错/对齐产物）不得驱动剪切；切片只产生 false。
    pub synthetic: bool,
    /// 合成词溯源；非合成词恒为 None。
    pub source_word_ids: Option<Vec<String>>,
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

/// 编辑行为。切片只实现 RippleAv（连带音视频联动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum EditBehavior {
    TextOnly,
    SubtitleOnly,
    RippleAv,
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
    /// 切点前后缓冲（毫秒），转采样时按资产采样率取整。
    pub handles_before_ms: i64,
    pub handles_after_ms: i64,
    /// 被更新的操作回填 supersede 链；活跃操作为 None。
    pub superseded_by: Option<String>,
    pub revision: i64,
    pub created_at: String,
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

/// 媒体资产状态机：imported → prepared → transcribed。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    Imported,
    Prepared,
    Transcribed,
}

/// 导入后的资产摘要（import_media 命令返回值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaAssetSummary {
    pub id: String,
    pub display_name: String,
    /// 时长（以 `audio_sample_rate` 为时基的采样数）。
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
