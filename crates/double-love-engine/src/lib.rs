mod compile;
mod contracts;
mod edit;
pub mod export;
mod media;
mod project;
mod rational;
mod segment;
mod sidecar;
mod storage;
mod task;
mod transcribe;

pub use compile::{CompileOptions, compile_rough_cut, output_to_source, source_to_output};
pub use contracts::{
    AssetStatus, EditBehavior, EditOperation, EditType, IrClip, MapSegment, MediaAssetSummary,
    TIMELINE_IR_SCHEMA_VERSION, TimelineIR, TranscriptSegment, WordAnchor,
};
pub use edit::{DEFAULT_HANDLES_MS, omit_words, restore_words};
pub use media::{FfmpegTools, PREPARED_SAMPLE_RATE, import_media};
pub use project::{ProjectError, ProjectSummary, create_project, open_project};
pub use rational::{FrameRate, Rational, Round, frame_to_samples, samples_to_frame};
pub use segment::segment_words;
pub use sidecar::{
    SIDECAR_PROTOCOL_VERSION, Sidecar, SidecarCommand, SidecarError, SidecarEvent, SidecarWord,
    resolve_python,
};
pub use storage::{MediaAssetRow, NewMediaAsset, NewTranscriptWord, ProjectStore, StorageError};
pub use task::{CancellationToken, ProgressSink, SharedSink, TaskRegistry};
pub use transcribe::{TranscribeConfig, start_transcription};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Success,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub cause: String,
    pub object_id: Option<String>,
    pub impact: String,
    pub blocks_export: bool,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct OperationCounts {
    pub total: u64,
    pub processed: u64,
    pub skipped: u64,
    pub failed: u64,
    pub unmatched: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct OutputArtifact {
    pub kind: String,
    pub path: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct ProgressEvent {
    pub task: String,
    pub phase: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Running,
    Paused,
    Failed,
    Partial,
    Succeeded,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct OperationResult<T> {
    pub status: OperationStatus,
    pub revision: Option<u64>,
    pub data: Option<T>,
    pub counts: OperationCounts,
    pub diagnostics: Vec<Diagnostic>,
    pub outputs: Vec<OutputArtifact>,
}

impl<T> OperationResult<T> {
    pub fn success(data: T) -> Self {
        Self {
            status: OperationStatus::Success,
            revision: None,
            data: Some(data),
            counts: OperationCounts::default(),
            diagnostics: Vec::new(),
            outputs: Vec::new(),
        }
    }

    pub fn failed(code: impl Into<String>, cause: impl Into<String>) -> Self {
        Self {
            status: OperationStatus::Failed,
            revision: None,
            data: None,
            counts: OperationCounts {
                failed: 1,
                ..OperationCounts::default()
            },
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Error,
                code: code.into(),
                cause: cause.into(),
                object_id: None,
                impact: "操作未产生可用结果".to_string(),
                blocks_export: true,
                suggested_action: None,
            }],
            outputs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticLevel, OperationResult, OperationStatus};

    #[test]
    fn failed_result_is_explicitly_failed() {
        let result: OperationResult<()> = OperationResult::failed("TEST", "test failure");
        assert_eq!(result.status, OperationStatus::Failed);
        assert_eq!(result.diagnostics[0].level, DiagnosticLevel::Error);
        assert!(result.diagnostics[0].blocks_export);
    }
}
