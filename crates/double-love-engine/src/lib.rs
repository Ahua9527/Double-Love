mod compile;
mod contracts;
mod diarize;
mod edit;
pub mod export;
mod main_track;
mod media;
mod model;
mod project;
mod rational;
mod segment;
mod sidecar;
mod speaker;
mod storage;
mod subtitle;
mod task;
mod timeline;
mod transcribe;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub use compile::{CompileOptions, compile_rough_cut, output_to_source, source_to_output};
pub use contracts::{
    AssetStatus, CanvasFit, CanvasSpec, CompatibilityReport, EditBehavior, EditOperation, EditType,
    IrClip, MainTrackClip, MapSegment, MediaAssetSummary, OutputMapSegment, ProjectExportPreview,
    ResolvedTimelineClip, RevisionHistoryEntry, SourceCut, SpeakerAssignment,
    SpeakerDiarizationResult, SpeakerIdentity, SpeakerMergeProposal, SpeakerNameAgentPayload,
    SpeakerNameProposal, SpeakerSegment, SubtitleCue, SubtitleStyle, TIMELINE_IR_SCHEMA_VERSION,
    TIMELINE_IR_V2_SCHEMA_VERSION, TimelineIR, TimelineIRv2, TimelineSource, TranscriptSegment,
    TranscriptViewData, WordAnchor,
};
pub use diarize::{DiarizeConfig, speaker_diarization_result, start_speaker_diarization};
pub use edit::{DEFAULT_HANDLES_MS, omit_words, restore_words};
pub use export::project::{export_project_ass_to, export_project_xmeml_to, preview_project_export};
pub use export::render::{ffmpeg_filter_graph, ffmpeg_supports_ass_filter, render_project_mp4_to};
pub use export::roughcut::{ExportOutcome, export_rough_cut, export_rough_cut_to};
pub use main_track::{
    append_full_main_track_asset, append_main_track_clip, compile_project_timeline,
    move_main_track_clip, remove_main_track_clip, split_main_track_clip, trim_main_track_clip,
};
pub use media::{FfmpegTools, PREPARED_SAMPLE_RATE, import_media, list_media_assets};
pub use model::{
    DoctorEnvironment, DoctorModelCheck, DoctorReport, FetchResponse, FetchStatus,
    MODEL_CATALOG_SCHEMA_VERSION, MODEL_INSTALLATIONS_SCHEMA_VERSION, ModelCatalog, ModelComponent,
    ModelDependency, ModelDescriptor, ModelDescriptorWithInstallation, ModelDownloadProgress,
    ModelError, ModelFetcher, ModelFile, ModelInstallState, ModelInstallation, ModelManager,
    write_fetch_response,
};
pub use project::{ProjectError, ProjectSummary, create_project, open_project};
pub use rational::{
    FrameRate, Rational, Round, convert_frame_rate, frame_to_samples, samples_to_frame,
};
pub use segment::{segment_words, transcript_view};
pub use sidecar::{
    SIDECAR_PROTOCOL_VERSION, Sidecar, SidecarCommand, SidecarError, SidecarEvent, SidecarPoll,
    SidecarSpeakerEmbedding, SidecarSpeakerSegment, SidecarWord, resolve_python,
};
pub use speaker::{
    agent_name_payload_preview, assign_words_to_speakers, local_name_proposals,
    merge_proposals_from_embeddings,
};
pub use storage::{MediaAssetRow, NewMediaAsset, NewTranscriptWord, ProjectStore, StorageError};
pub use subtitle::{apply_speaker_names, build_subtitle_cues, export_ass};
pub use task::{CancellationToken, ProgressSink, SharedSink, TaskRegistry};
pub use timeline::compile_main_track;
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
