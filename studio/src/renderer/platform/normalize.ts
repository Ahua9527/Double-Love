import type { OperationResult } from "../../../../bindings/OperationResult";
import type { CanvasSpec } from "../../../../bindings/CanvasSpec";
import type { FrameRate } from "../../../../bindings/FrameRate";
import type { SubtitleStyle } from "../../../../bindings/SubtitleStyle";
export type { ModelQueueEntry } from "../../../../bindings/ModelQueueEntry";
export type { ModelQueueSnapshot } from "../../../../bindings/ModelQueueSnapshot";

export type ThemeMode = "light" | "dark" | "system";
export type TimecodePrecision = "frame" | "millisecond";
export type ProjectLibraryView = "grid" | "list";
export type ModelInstallState =
  | "not_installed"
  | "queued"
  | "downloading"
  | "paused"
  | "verifying"
  | "installed"
  | "corrupt"
  | "failed";
export type ModelDownloadSource =
  | "modelscope"
  | "huggingface"
  | "bundled"
  | "official";
export type ModelUiRole = "primary" | "dependency" | "bundled" | "legacy";

export interface AppInfo {
  name: string;
  version: string;
}

export type UpdateStage =
  | "idle"
  | "checking-update"
  | "update-available"
  | "update-not-available"
  | "download-progress"
  | "update-downloaded"
  | "error";

export interface UpdateStatus {
  stage: UpdateStage;
  version?: string;
  percent?: number;
  error?: string;
}

export interface AppPreferencesV1 {
  schema_version: number;
  theme: ThemeMode;
  restore_last_project: boolean;
  timecode_precision: TimecodePrecision;
  project_library_view: ProjectLibraryView;
  history_limit: number | null;
  transcript_section_tint: boolean;
  cjk_spacing: boolean;
  default_subtitle_style: SubtitleStyle | null;
  model_root: string;
  model_endpoint: string;
  default_asr_model: string;
  onboarding_version: number;
  onboarding_completed: boolean;
  recent_projects: RecentProjectRecord[];
}

export interface PreferencesPatch {
  theme?: ThemeMode;
  restore_last_project?: boolean;
  project_library_view?: ProjectLibraryView;
  history_limit?: number | null;
  transcript_section_tint?: boolean;
  cjk_spacing?: boolean;
  default_subtitle_style?: SubtitleStyle | null;
  default_asr_model?: string;
  model_endpoint?: string;
  model_root?: string;
}

export interface RecentProject {
  project_id: string | null;
  root: string;
  display_name: string;
  last_opened_at: string;
  exists: boolean;
  created_at: string | null;
  modified_at: string | null;
  canvas: CanvasSpec | null;
  output_rate: FrameRate | null;
}

export interface RecentProjectRecord {
  project_id: string | null;
  root: string;
  display_name: string;
  last_opened_at: string;
}

export interface SystemProfile {
  memory_bytes: number | bigint;
  architecture: string;
  os_version: string;
  free_model_bytes: number | bigint;
  recommended_asr_model: string;
}

export interface ModelDependency {
  model_id: string;
  required: boolean;
  reason?: string;
}

export interface ModelDescriptor {
  id: string;
  label: string;
  kind: "asr" | "aligner" | "speaker" | "vad" | string;
  ui_role: ModelUiRole;
  download_source: ModelDownloadSource;
  revision: string;
  size_bytes: number | bigint;
  memory_bytes: number | bigint;
  license: string;
  description?: string;
  dependencies: ModelDependency[];
  replaces_model_id?: string | null;
  state: ModelInstallState;
  installed_revision?: string | null;
  downloaded_bytes?: number | bigint | null;
  error?: string | null;
  can_remove?: boolean;
}

export interface LegacyModelCleanupItem {
  model_id: string;
  display_name: string;
  bytes_to_free: number | bigint;
  reason: string | null;
}

export interface LegacyModelCleanupPreview {
  target_model_id: string;
  bytes_to_free: number | bigint;
  removable: LegacyModelCleanupItem[];
  retained: LegacyModelCleanupItem[];
}

export interface ModelInstallation {
  model_id: string;
  state: ModelInstallState;
  revision: string;
  downloaded_bytes: number | bigint;
  total_bytes: number | bigint;
  error?: string | null;
  updated_at?: string;
}

export interface ModelDownloadProgress {
  model_id: string;
  state: ModelInstallState;
  completed_bytes: number | bigint;
  total_bytes: number | bigint;
  message?: string | null;
}

export interface DoctorReport {
  generated_at: string;
  app_version: string;
  architecture: string;
  os_version: string;
  ffmpeg: string;
  libass: string;
  model_integrity: Array<{
    model_id: string;
    state: string;
    detail?: string | null;
  }>;
  capability_checks: Array<DoctorCapabilityCheck>;
  free_disk_bytes: number | bigint;
  logs_directory: string;
  offline_runtime: string;
  sanitized_text?: string;
}

export type DoctorCapabilityStatus =
  | "ready"
  | "warning"
  | "blocked"
  | "not_run";

export interface DoctorCapabilityCheck {
  id: string;
  status: DoctorCapabilityStatus;
  detail: string;
  suggested_action?: string | null;
}

export interface OnboardingState {
  version: number;
  completed: boolean;
  step: 1 | 2 | 3;
}

export type RawModelDescriptor = Partial<ModelDescriptor> & {
  display_name?: string;
  component?: string;
  files?: Array<{ size_bytes?: number | bigint }>;
  min_memory_bytes?: number | bigint | null;
  bundled?: boolean;
  replaces_model_id?: string | null;
};

export type RawModelInstallation = Partial<ModelInstallation> & {
  bytes_downloaded?: number | bigint;
  bytes_total?: number | bigint;
  staging_id?: string | null;
  last_error_code?: string | null;
  last_error_message?: string | null;
};

export type RawModelSnapshot = {
  descriptor?: RawModelDescriptor;
  installation?: RawModelInstallation;
};

function componentKind(component: string | undefined): ModelDescriptor["kind"] {
  if (component === "forced_aligner" || component === "aligner")
    return "aligner";
  if (component === "vad") return "vad";
  if (component === "speaker") return "speaker";
  return "asr";
}

function sumModelFiles(files: RawModelDescriptor["files"]): number {
  return (files ?? []).reduce(
    (total, file) => total + Number(file.size_bytes ?? 0),
    0,
  );
}

function modelUiRole(raw: RawModelDescriptor): ModelUiRole {
  if (
    raw.ui_role === "primary" ||
    raw.ui_role === "dependency" ||
    raw.ui_role === "bundled" ||
    raw.ui_role === "legacy"
  ) {
    return raw.ui_role;
  }
  return raw.bundled ? "bundled" : "primary";
}

export function normalizeModelInstallation(
  raw: RawModelInstallation,
  descriptor?: RawModelDescriptor,
): ModelInstallation {
  const state = raw.state ?? "not_installed";
  return {
    model_id: raw.model_id ?? descriptor?.id ?? "",
    state,
    revision: raw.revision ?? descriptor?.revision ?? "",
    downloaded_bytes: raw.downloaded_bytes ?? raw.bytes_downloaded ?? 0,
    total_bytes:
      raw.total_bytes ?? raw.bytes_total ?? sumModelFiles(descriptor?.files),
    error: raw.error ?? raw.last_error_message ?? null,
    updated_at: raw.updated_at,
  };
}

export function normalizeModelDescriptor(
  rawValue: RawModelDescriptor | RawModelSnapshot,
): ModelDescriptor {
  const raw =
    "descriptor" in rawValue && rawValue.descriptor
      ? rawValue.descriptor
      : (rawValue as RawModelDescriptor);
  const installation =
    "installation" in rawValue && rawValue.installation
      ? rawValue.installation
      : undefined;
  const id = raw.id ?? "";
  const catalogSize = raw.size_bytes ?? sumModelFiles(raw.files);
  const installedSize = installation?.bytes_total ?? installation?.total_bytes ?? 0;
  return {
    id,
    label: raw.label ?? raw.display_name ?? id,
    kind: raw.kind ?? componentKind(raw.component),
    ui_role: modelUiRole(raw),
    download_source:
      raw.download_source ?? (raw.bundled ? "bundled" : "huggingface"),
    revision: raw.revision ?? "",
    size_bytes: Number(catalogSize) > 0 ? catalogSize : installedSize,
    memory_bytes: raw.memory_bytes ?? raw.min_memory_bytes ?? 0,
    license: raw.license ?? "未知许可",
    description: raw.description,
    dependencies: raw.dependencies ?? [],
    replaces_model_id: raw.replaces_model_id ?? null,
    state: installation?.state ?? raw.state ?? "not_installed",
    installed_revision:
      installation?.state === "installed"
        ? (installation.revision ?? raw.installed_revision)
        : raw.installed_revision,
    downloaded_bytes:
      installation?.downloaded_bytes ??
      installation?.bytes_downloaded ??
      raw.downloaded_bytes ??
      0,
    error:
      installation?.error ??
      installation?.last_error_message ??
      raw.error ??
      null,
    can_remove: raw.can_remove ?? (!raw.bundled && raw.component !== "vad"),
  };
}

export function normalizeModelProgress(
  raw: Partial<ModelDownloadProgress> & {
    bytes_downloaded?: number | bigint;
    bytes_total?: number | bigint;
  },
): ModelDownloadProgress {
  return {
    model_id: raw.model_id ?? "",
    state: raw.state ?? "not_installed",
    completed_bytes: raw.completed_bytes ?? raw.bytes_downloaded ?? 0,
    total_bytes: raw.total_bytes ?? raw.bytes_total ?? 0,
    message: raw.message ?? null,
  };
}

export function normalizeLegacyModelCleanupPreview(
  raw: Partial<LegacyModelCleanupPreview>,
): LegacyModelCleanupPreview {
  const item = (value: Partial<LegacyModelCleanupItem>): LegacyModelCleanupItem => ({
    model_id: value.model_id ?? "",
    display_name: value.display_name ?? value.model_id ?? "旧模型",
    bytes_to_free: value.bytes_to_free ?? 0,
    reason: value.reason ?? null,
  });
  const removable = Array.isArray(raw.removable) ? raw.removable.map(item) : [];
  const retained = Array.isArray(raw.retained) ? raw.retained.map(item) : [];
  return {
    target_model_id: raw.target_model_id ?? "",
    bytes_to_free: raw.bytes_to_free ?? 0,
    removable,
    retained,
  };
}

export function normalizeDoctorReport(
  raw: Partial<DoctorReport> & {
    model_checks?: Array<{
      model_id: string;
      state: string;
      error_code?: string | null;
    }>;
    ffmpeg_available?: boolean;
    libass_available?: boolean;
    model_root_available?: boolean;
    free_model_bytes?: number | bigint;
    asr_runtime_ready?: boolean;
    speaker_runtime_ready?: boolean;
    warnings?: string[];
    capability_checks?: Array<Partial<DoctorCapabilityCheck>>;
  },
): DoctorReport {
  return {
    generated_at: raw.generated_at ?? new Date().toISOString(),
    app_version: raw.app_version ?? "0.2.0",
    architecture: raw.architecture ?? "unknown",
    os_version: raw.os_version ?? "unknown",
    ffmpeg: raw.ffmpeg ?? (raw.ffmpeg_available ? "可用" : "不可用"),
    libass: raw.libass ?? (raw.libass_available ? "可用" : "不可用"),
    model_integrity:
      raw.model_integrity ??
      (raw.model_checks ?? []).map((model) => ({
        model_id: model.model_id,
        state: model.state,
        detail: model.error_code ?? null,
      })),
    capability_checks: (raw.capability_checks ?? []).map((check) => ({
      id: check.id ?? "unknown",
      status: check.status ?? "not_run",
      detail: check.detail ?? "未执行诊断。",
      suggested_action: check.suggested_action ?? null,
    })),
    free_disk_bytes: raw.free_disk_bytes ?? raw.free_model_bytes ?? 0,
    logs_directory: raw.logs_directory ?? "应用日志目录",
    offline_runtime:
      raw.offline_runtime ??
      (raw.asr_runtime_ready && raw.speaker_runtime_ready
        ? "ASR 与说话人运行时可用"
        : `ASR ${raw.asr_runtime_ready ? "可用" : "不可用"} · 说话人 ${raw.speaker_runtime_ready ? "可用" : "不可用"}`),
    sanitized_text: raw.sanitized_text,
  };
}

export function mapOperation<T, U>(
  result: OperationResult<T>,
  map: (value: T) => U,
): OperationResult<U> {
  return { ...result, data: result.data === null ? null : map(result.data) };
}
