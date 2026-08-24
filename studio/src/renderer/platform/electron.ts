import type { CanvasSpec } from "../../../../bindings/CanvasSpec";
import type { EditOperation } from "../../../../bindings/EditOperation";
import type { ExportOutcome } from "../../../../bindings/ExportOutcome";
import type { FrameRate } from "../../../../bindings/FrameRate";
import type { MainTrackClip } from "../../../../bindings/MainTrackClip";
import type { MediaAssetSummary } from "../../../../bindings/MediaAssetSummary";
import type { OperationResult } from "../../../../bindings/OperationResult";
import type { ProjectExportPreview } from "../../../../bindings/ProjectExportPreview";
import type { ProjectSummary } from "../../../../bindings/ProjectSummary";
import type { RevisionHistoryEntry } from "../../../../bindings/RevisionHistoryEntry";
import type { SpeakerDiarizationResult } from "../../../../bindings/SpeakerDiarizationResult";
import type { SpeakerIdentity } from "../../../../bindings/SpeakerIdentity";
import type { SpeakerNameAgentPayload } from "../../../../bindings/SpeakerNameAgentPayload";
import type { SpeakerNameProposal } from "../../../../bindings/SpeakerNameProposal";
import type { SubtitleStyle } from "../../../../bindings/SubtitleStyle";
import type { TimelineIRv2 } from "../../../../bindings/TimelineIRv2";
import type { TranscriptViewData } from "../../../../bindings/TranscriptViewData";
import {
  mapOperation,
  normalizeDoctorReport,
  normalizeLegacyModelCleanupPreview,
  normalizeModelDescriptor,
  normalizeModelInstallation,
  type AppInfo,
  type AppPreferencesV1,
  type DoctorReport,
  type LegacyModelCleanupPreview,
  type OnboardingState,
  type PreferencesPatch,
  type RawModelDescriptor,
  type RawModelInstallation,
  type RawModelSnapshot,
  type RecentProject,
  type SystemProfile,
  type UpdateStatus,
} from "./normalize";

export * from "./normalize";

export const isDesktop = true;
export const platformKind = "electron" as const;

export interface HostResponse {
  v: number;
  id: string;
  status: "ok" | "error";
  result?: { type: string; data?: unknown };
  error?: { code: string; message: string };
}

export interface GrantToken {
  token: string;
  displayName?: string;
}

interface DoubleLoveBridge {
  hostHealth(): Promise<unknown>;
  openSettings(): Promise<void>;
  getAppInfo(): Promise<AppInfo>;
  createProject(options: {
    name: string;
  }): Promise<HostResponse>;
  trashProject(projectId: string): Promise<HostResponse>;
  onPrepareQuit(callback: () => void | Promise<void>): () => void;
  updates: {
    check(): Promise<UpdateStatus>;
    download(): Promise<UpdateStatus>;
    install(): Promise<UpdateStatus>;
  };
  dialogs: {
    pickDirectory(options: {
      title: string;
      kind: "project-open" | "project-parent" | "model-root" | "model-import";
    }): Promise<GrantToken | null>;
    pickMediaFile(): Promise<GrantToken | null>;
    grantDroppedMedia(files: File[]): Promise<GrantToken[]>;
    pickExportPath(options: {
      defaultName: string;
      kind: "xml" | "ass" | "mp4";
    }): Promise<GrantToken | null>;
  };
  player: {
    setBounds(bounds: { x: number; y: number; width: number; height: number }): Promise<void>;
    loadTimeline(clips: NativePlayerClip[], seconds: number): Promise<void>;
    setSubtitle(config: NativeSubtitleConfig): Promise<void>;
    setPresentation(config: NativePresentationConfig): Promise<void>;
    play(): Promise<void>;
    pause(): Promise<void>;
    seek(seconds: number): Promise<void>;
    dispose(): Promise<void>;
    onState(callback: (state: NativePlayerState) => void): () => void;
  };
  invoke(name: string, payload?: unknown): Promise<HostResponse>;
  onEvent(channel: string, callback: (payload: unknown) => void): () => void;
}

export interface NativePlayerState {
  state: "loading" | "ready" | "playing" | "waiting" | "paused" | "ended" | "error";
  seconds: number;
  duration: number;
  rate: number;
  ready_for_display: boolean;
  error: string;
}

export interface NativePlayerClip {
  assetId: string;
  sourceStartSeconds: number;
  sourceDurationSeconds: number;
  outputStartSeconds: number;
  outputDurationSeconds: number;
}

export interface NativeSubtitleConfig {
  text: string;
  canvasWidth: number;
  fontFamily: string;
  fontSize: number;
  textColor: string;
  outlineColor: string;
  outlineWidth: number;
  shadowColor: string;
  shadowX: number;
  shadowY: number;
  shadowBlur: number;
  backgroundColor: string;
  radius: number;
  paddingX: number;
  paddingY: number;
  x: number;
  y: number;
  maxWidth: number;
}

export interface NativePresentationConfig {
  fit: "contain" | "cover";
  canvasWidth: number;
  canvasHeight: number;
  positionX: number;
  positionY: number;
  scale: number;
  rotation: number;
  opacity: number;
  background: string;
}

function bridge(): DoubleLoveBridge {
  // SAFETY: desktop.ts selects this adapter only when preload exposed the declared bridge.
  return (window as unknown as { doubleLove: DoubleLoveBridge }).doubleLove;
}

function failedOperation<T>(code: string, message: string): OperationResult<T> {
  // SAFETY: OperationResult's ts-rs integer types are bigint, but its JSON wire format uses numbers.
  const integer = (value: number) => value as unknown as bigint;
  return {
    status: "failed",
    revision: null,
    data: null,
    counts: {
      total: integer(0),
      processed: integer(0),
      skipped: integer(0),
      failed: integer(1),
      unmatched: integer(0),
    },
    diagnostics: [
      {
        level: "error",
        code,
        cause: message,
        object_id: null,
        impact: "操作未产生可用结果",
        blocks_export: true,
        suggested_action: null,
      },
    ],
    outputs: [],
  };
}

function successOperation<T>(data: T): OperationResult<T> {
  // SAFETY: OperationResult's ts-rs integer types are bigint, but its JSON wire format uses numbers.
  const integer = (value: number) => value as unknown as bigint;
  return {
    status: "success",
    revision: null,
    data,
    counts: {
      total: integer(0),
      processed: integer(0),
      skipped: integer(0),
      failed: integer(0),
      unmatched: integer(0),
    },
    diagnostics: [],
    outputs: [],
  };
}

function operationFromResponse<T>(response: HostResponse): OperationResult<T> {
  if (response.status === "ok" && response.result?.type === "invoke") {
    return response.result.data as OperationResult<T>;
  }
  return failedOperation(
    response.error?.code ?? "INVALID_HOST_RESPONSE",
    response.error?.message ?? "Invalid host response",
  );
}

export async function invokeOperation<T>(
  name: string,
  payload?: unknown,
): Promise<OperationResult<T>> {
  return operationFromResponse<T>(await bridge().invoke(name, payload));
}

export function listen<T>(
  channel: string,
  callback: (event: { payload: T }) => void,
): Promise<() => void> {
  return Promise.resolve(
    bridge().onEvent(channel, (payload) => callback({ payload: payload as T })),
  );
}

export function projectOpen(grantToken: string) {
  return invokeOperation<ProjectSummary>("project_open", { grantToken });
}
export function createProject(options: {
  name: string;
}) {
  return bridge()
    .createProject(options)
    .then((response) => operationFromResponse<ProjectSummary>(response));
}
export function recentProjectOpen(projectId: string) {
  return invokeOperation<ProjectSummary>("recent_project_open", { projectId });
}
export function trashProject(projectId: string) {
  return bridge()
    .trashProject(projectId)
    .then((response) => operationFromResponse<null>(response));
}
export function projectCheckpoint() {
  return invokeOperation<bigint | null>("project_checkpoint");
}
export function projectClose() {
  return invokeOperation<null>("project_close");
}
export function onPrepareQuit(callback: () => void | Promise<void>) {
  return bridge().onPrepareQuit(callback);
}
export function assetsList() {
  return invokeOperation<MediaAssetSummary[]>("assets_list");
}
export function importMedia(grantToken: string) {
  return invokeOperation<MediaAssetSummary>("import_media", { grantToken });
}
export function removeMediaAsset(assetId: string) {
  return invokeOperation<import("../../../../bindings/MediaAssetRemoval").MediaAssetRemoval>(
    "media_asset_remove",
    { assetId },
  );
}
export function transcriptGet(assetId: string) {
  return invokeOperation<TranscriptViewData>("transcript_get", { assetId });
}
export function transcribeStart(
  assetId: string,
  model: string,
  language: string,
) {
  return invokeOperation<{ task_id: string }>("transcribe_start", {
    assetId,
    model,
    language,
  });
}
export function taskCancel(taskId: string) {
  return invokeOperation<{ task_id: string }>("task_cancel", { taskId });
}
export function projectRevision() {
  return invokeOperation<bigint>("project_revision");
}
export function projectHistory(limit = 80) {
  return invokeOperation<RevisionHistoryEntry[]>("project_history", { limit });
}
export function projectRestoreRevision(revision: number) {
  return invokeOperation<{ restored_revision: bigint; revision: bigint }>(
    "project_restore_revision",
    { revision },
  );
}
export function editOmit(
  assetId: string,
  startOrdinal: number,
  endOrdinal: number,
) {
  return invokeOperation<EditOperation>("edit_omit", {
    assetId,
    startOrdinal,
    endOrdinal,
  });
}
export function editRestore(
  operationId: string,
  startOrdinal: number,
  endOrdinal: number,
) {
  return invokeOperation<EditOperation>("edit_restore", {
    operationId,
    startOrdinal,
    endOrdinal,
  });
}
export function roughcutPreview(assetId: string) {
  return invokeOperation<ExportOutcome>("roughcut_preview", { assetId });
}
export function exportRoughcutApply(assetId: string, grantToken: string) {
  return invokeOperation<ExportOutcome>("export_roughcut_apply", {
    assetId,
    grantToken,
  });
}
export function timelineGet() {
  return invokeOperation<TimelineIRv2>("timeline_get");
}
export function mainTrackList() {
  return invokeOperation<MainTrackClip[]>("main_track_list");
}
export function mainTrackAppendFull(assetId: string) {
  return invokeOperation<MainTrackClip>("main_track_append_full", { assetId });
}
export function mainTrackInsertAssets(assetIds: string[], beforeClipId: string | null) {
  return invokeOperation<MainTrackClip[]>("main_track_insert_assets", {
    assetIds,
    beforeClipId,
  });
}
export function mainTrackMove(clipId: string, beforeClipId: string | null) {
  return invokeOperation<null>("main_track_move", { clipId, beforeClipId });
}
export function mainTrackTrim(
  clipId: string,
  sourceInFrame: number,
  sourceOutFrame: number,
) {
  return invokeOperation<MainTrackClip>("main_track_trim", {
    clipId,
    sourceInFrame,
    sourceOutFrame,
  });
}
export function mainTrackSplit(clipId: string, sourceAtFrame: number) {
  return invokeOperation<MainTrackClip[]>("main_track_split", {
    clipId,
    sourceAtFrame,
  });
}
export function mainTrackRemove(clipId: string) {
  return invokeOperation<null>("main_track_remove", { clipId });
}
export function canvasGet() {
  return invokeOperation<CanvasSpec>("canvas_get");
}
export function canvasSet(canvas: CanvasSpec) {
  return invokeOperation<CanvasSpec>("canvas_set", { canvas });
}
export function outputRateGet() {
  return invokeOperation<FrameRate | null>("output_rate_get");
}
export function outputRateSet(rate: FrameRate | null) {
  return invokeOperation<FrameRate | null>("output_rate_set", { rate });
}
export function subtitleStyleGet() {
  return invokeOperation<SubtitleStyle>("subtitle_style_get");
}
export function subtitleStyleSet(style: SubtitleStyle) {
  return invokeOperation<SubtitleStyle>("subtitle_style_set", { style });
}
export function applyDefaultSubtitleStyle() {
  return invokeOperation<SubtitleStyle>("apply_default_subtitle_style");
}
export function speakerList() {
  return invokeOperation<SpeakerIdentity[]>("speaker_list");
}
export function speakerDiarizeStart(assetId: string) {
  return invokeOperation<{ task_id: string }>("speaker_diarize_start", {
    assetId,
  });
}
export function speakerDiarizationGet(assetId: string) {
  return invokeOperation<SpeakerDiarizationResult>("speaker_diarization_get", {
    assetId,
  });
}
export function speakerNameProposals(assetId: string) {
  return invokeOperation<SpeakerNameProposal[]>("speaker_name_proposals", {
    assetId,
  });
}
export function speakerAgentPayloadPreview(assetId: string, speakerId: string) {
  return invokeOperation<SpeakerNameAgentPayload>(
    "speaker_agent_payload_preview",
    { assetId, speakerId },
  );
}
export function speakerNameConfirm(speakerId: string, displayName: string) {
  return invokeOperation<SpeakerIdentity>("speaker_name_confirm", {
    speakerId,
    displayName,
    confirmed: true,
  });
}
export function speakerMergeConfirm(
  keepSpeakerId: string,
  mergeSpeakerId: string,
) {
  return invokeOperation<SpeakerIdentity>("speaker_merge_confirm", {
    keepSpeakerId,
    mergeSpeakerId,
    confirmed: true,
  });
}
export function projectExportPreview() {
  return invokeOperation<ProjectExportPreview>("project_export_preview");
}
export function projectExportXmemlApply(grantToken: string) {
  return invokeOperation<ProjectExportPreview>("project_export_xmeml_apply", {
    grantToken,
  });
}
export function projectExportAssApply(grantToken: string) {
  return invokeOperation<ProjectExportPreview>("project_export_ass_apply", {
    grantToken,
  });
}
export function projectRenderMp4Apply(grantToken: string) {
  return invokeOperation<ProjectExportPreview>("project_render_mp4_apply", {
    grantToken,
  });
}

export async function pickDirectory(
  title: string,
  kind?: "project-open" | "model-root" | "model-import",
) {
  if (!kind)
    throw new TypeError("Electron directory dialogs require a grant kind");
  return (await bridge().dialogs.pickDirectory({ title, kind }))?.token ?? null;
}

export async function pickMediaFile() {
  return (await bridge().dialogs.pickMediaFile())?.token ?? null;
}
export function grantDroppedMedia(files: File[]) {
  return bridge().dialogs.grantDroppedMedia(files);
}
export const playerSetBounds = (bounds: { x: number; y: number; width: number; height: number }) => bridge().player.setBounds(bounds);
export const playerLoadTimeline = (clips: NativePlayerClip[], seconds: number) => bridge().player.loadTimeline(clips, seconds);
export const playerSetSubtitle = (config: NativeSubtitleConfig) => bridge().player.setSubtitle(config);
export const playerSetPresentation = (config: NativePresentationConfig) => bridge().player.setPresentation(config);
export const playerPlay = () => bridge().player.play();
export const playerPause = () => bridge().player.pause();
export const playerSeek = (seconds: number) => bridge().player.seek(seconds);
export const playerDispose = () => bridge().player.dispose();
export const onPlayerState = (callback: (state: NativePlayerState) => void) => bridge().player.onState(callback);

export async function pickSavePath(defaultName: string) {
  return (
    (await bridge().dialogs.pickExportPath({ defaultName, kind: "xml" }))
      ?.token ?? null
  );
}

export async function pickProjectExportPath(
  defaultName: string,
  kind: "xml" | "ass" | "mp4",
) {
  return (
    (await bridge().dialogs.pickExportPath({ defaultName, kind }))?.token ??
    null
  );
}

export async function settingsOpen(): Promise<OperationResult<null>> {
  // 设置窗口由 Electron main 管理，不走 host。
  await bridge().openSettings();
  return successOperation(null);
}
export function getAppInfo() {
  return bridge().getAppInfo();
}
export function updateCheck() {
  return bridge().updates.check();
}
export function updateDownload() {
  return bridge().updates.download();
}
export function updateInstall() {
  return bridge().updates.install();
}
export function preferencesGet() {
  return invokeOperation<AppPreferencesV1>("preferences_get");
}
export function preferencesUpdate(patch: PreferencesPatch) {
  return invokeOperation<AppPreferencesV1>("preferences_update", { patch });
}
export function historyLimitPreview(limit: number | null) {
  return invokeOperation<number>("history_limit_preview", { limit });
}
export function recentProjectsList() {
  return invokeOperation<RecentProject[]>("recent_projects_list");
}
export function recentProjectForget(root: string) {
  return invokeOperation<null>("recent_project_forget", { root });
}
export function systemProfile() {
  return invokeOperation<SystemProfile>("system_profile");
}
export function modelCatalog() {
  return invokeOperation<unknown>("model_catalog").then((result) =>
    mapOperation(result, (value) =>
      Array.isArray(value)
        ? value.map((item) =>
            normalizeModelDescriptor(
              item as RawModelDescriptor | RawModelSnapshot,
            ),
          )
        : [],
    ),
  );
}
export function modelQueueGet() {
  return invokeOperation<import("../../../../bindings/ModelQueueSnapshot").ModelQueueSnapshot>(
    "model_queue_get",
  );
}
export function modelInstall(
  modelId: string,
  acceptNoncommercialLicense = false,
) {
  return modelOperation("model_install", modelId, {
    acceptNoncommercialLicense,
  });
}
export function modelPause(modelId: string) {
  return modelOperation("model_pause", modelId);
}
export function modelResume(modelId: string) {
  return modelOperation("model_resume", modelId);
}
export function modelCancel(modelId: string) {
  return modelOperation("model_cancel", modelId);
}
export function modelVerify(modelId: string) {
  return modelOperation("model_verify", modelId);
}
export function modelRemove(modelId: string) {
  return modelOperation("model_remove", modelId);
}

export function modelLegacyCleanupPreview(modelId: string) {
  return invokeOperation<unknown>("model_legacy_cleanup_preview", { modelId }).then(
    (result) =>
      mapOperation(result, (value) =>
        normalizeLegacyModelCleanupPreview(
          value as Partial<LegacyModelCleanupPreview>,
        ),
      ),
  );
}

export function modelLegacyCleanupApply(modelId: string) {
  return invokeOperation<unknown>("model_legacy_cleanup_apply", {
    modelId,
    confirmed: true,
  }).then((result) =>
    mapOperation(result, (value) =>
      normalizeLegacyModelCleanupPreview(
        value as Partial<LegacyModelCleanupPreview>,
      ),
    ),
  );
}

export function modelImportFolder(
  modelId: string,
  grantToken: string,
  acceptNoncommercialLicense = false,
) {
  return invokeOperation<unknown>("model_import_folder", {
    modelId,
    grantToken,
    acceptNoncommercialLicense,
  }).then((result) =>
    mapOperation(result, (value) =>
      normalizeModelInstallation(value as RawModelInstallation),
    ),
  );
}

function modelOperation(
  name: string,
  modelId: string,
  extraPayload?: Record<string, unknown>,
) {
  return invokeOperation<unknown>(name, { modelId, ...extraPayload }).then((result) =>
    mapOperation(result, (value) =>
      normalizeModelInstallation(value as RawModelInstallation),
    ),
  );
}

export function modelReveal(modelId: string) {
  return invokeOperation<null>(
    "model_reveal",
    { modelId },
  );
}
export function doctorRun(depth: "quick" | "deep" = "quick") {
  return invokeOperation<unknown>("doctor_run", { depth }).then((result) =>
    mapOperation(result, (value) =>
      normalizeDoctorReport(
        value as Partial<DoctorReport> & {
          model_checks?: Array<{
            model_id: string;
            state: string;
            error_code?: string | null;
          }>;
          ffmpeg_available?: boolean;
          libass_available?: boolean;
          model_root_available?: boolean;
          warnings?: string[];
          capability_checks?: Array<{
            id?: string;
            status?: "ready" | "warning" | "blocked" | "not_run";
            detail?: string;
            suggested_action?: string | null;
          }>;
        },
      ),
    ),
  );
}
export function diagnosticsRevealLogs() {
  return invokeOperation<null>("diagnostics_reveal_logs");
}
export function onboardingGet() {
  return invokeOperation<OnboardingState>("onboarding_get");
}
export function onboardingComplete(defaultAsrModel?: string) {
  return invokeOperation<OnboardingState>(
    "onboarding_complete",
    defaultAsrModel ? { defaultAsrModel } : undefined,
  );
}
export function onboardingReset() {
  return invokeOperation<OnboardingState>("onboarding_reset");
}
export function editUndo() {
  return invokeOperation<null>("edit_undo");
}
export function editRedo() {
  return invokeOperation<null>("edit_redo");
}
