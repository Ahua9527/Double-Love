import type {
  ModelDependency,
  ModelDescriptor,
  ModelInstallState,
} from "./platform/desktop";
import { num } from "./utils";

export type ResolvedModelDependency = ModelDependency & {
  model: ModelDescriptor;
};

const ACTIVE_STATES: ModelInstallState[] = [
  "queued",
  "downloading",
  "verifying",
];
const ATTENTION_STATES: ModelInstallState[] = [
  "paused",
  "failed",
  "corrupt",
];

export interface ModelDownloadGroup {
  root: ModelDescriptor;
  members: ModelDescriptor[];
  current: ModelDescriptor | null;
  completedBytes: number;
  totalBytes: number;
  percent: number;
  active: boolean;
  attention: boolean;
  phase: string;
}

export interface ModelDownloadSize {
  completedBytes: number;
  totalBytes: number;
}

function isActive(state: ModelInstallState): boolean {
  return ACTIVE_STATES.includes(state);
}

function isAttention(state: ModelInstallState): boolean {
  return ATTENTION_STATES.includes(state);
}

function dependencyClosure(
  root: ModelDescriptor,
  byId: Map<string, ModelDescriptor>,
): ModelDescriptor[] {
  const result: ModelDescriptor[] = [];
  const visited = new Set<string>();
  const visit = (model: ModelDescriptor) => {
    if (visited.has(model.id)) return;
    visited.add(model.id);
    result.push(model);
    for (const dependency of model.dependencies) {
      if (!dependency.required) continue;
      const resolved = byId.get(dependency.model_id);
      if (resolved) visit(resolved);
    }
  };
  visit(root);
  return result;
}

function currentModel(members: ModelDescriptor[]): ModelDescriptor | null {
  const priority: ModelInstallState[] = [
    "downloading",
    "verifying",
    "queued",
    "paused",
    "failed",
    "corrupt",
  ];
  for (const state of priority) {
    const match = members.find((model) => model.state === state);
    if (match) return match;
  }
  return null;
}

function phaseLabel(model: ModelDescriptor, state: ModelInstallState): string {
  if (state === "downloading") {
    if (model.kind === "aligner") return "正在下载 ForcedAligner";
    if (model.kind === "vad") return "正在下载 VAD";
    return `正在下载 ${model.label}`;
  }
  if (state === "verifying") return `正在校验 ${model.label}`;
  if (state === "queued") return `等待下载 ${model.label}`;
  if (state === "paused") return `下载已暂停 · ${model.label}`;
  if (state === "failed" || state === "corrupt") {
    return `下载失败 · ${model.label}`;
  }
  return model.label;
}

function completedBytes(model: ModelDescriptor): number {
  const total = Math.max(0, num(model.size_bytes));
  if (model.state === "installed") return total;
  return Math.min(total, Math.max(0, num(model.downloaded_bytes ?? 0)));
}

function aggregateSize(members: ModelDescriptor[]): ModelDownloadSize {
  return members.reduce(
    (size, model) => ({
      completedBytes: size.completedBytes + completedBytes(model),
      totalBytes: size.totalBytes + Math.max(0, num(model.size_bytes)),
    }),
    { completedBytes: 0, totalBytes: 0 },
  );
}

/** Format the shared download byte display used by Settings and BackgroundTasks. */
export function formatDownloadSize(value: number | bigint): string {
  const bytes = Math.max(0, Number(value));
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 ** 2) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) {
    const megabytes = bytes / 1024 ** 2;
    return `${megabytes >= 10 ? Math.round(megabytes) : megabytes.toFixed(1)} MB`;
  }
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function formatDownloadSizePair(
  completedBytes: number | bigint,
  totalBytes: number | bigint,
): string {
  return `${formatDownloadSize(completedBytes)} / ${formatDownloadSize(totalBytes)}`;
}

/**
 * Resolve one install request into a single weighted progress group. The desktop
 * service downloads required dependencies first, so the renderer can derive the
 * current phase from the existing model snapshots without creating another task.
 */
export function modelDownloadGroup(
  root: ModelDescriptor,
  models: ModelDescriptor[],
): ModelDownloadGroup | null {
  const byId = new Map(models.map((model) => [model.id, model]));
  const members = dependencyClosure(root, byId);
  const relevant = members.some(
    (model) => isActive(model.state) || isAttention(model.state),
  );
  const rootParticipates = isActive(root.state) || isAttention(root.state);
  // An installed primary with a missing dependency is an inline repair hint,
  // not an active download group; keep that state in visibleModelDependencies.
  // A dependency can also be installed manually; do not attach that standalone
  // task to every primary model that happens to reference it.
  if (!relevant || !rootParticipates) {
    return null;
  }
  const size = aggregateSize(members);
  const current = currentModel(members);
  const active = members.some((model) => isActive(model.state));
  const attention = !active && members.some((model) => isAttention(model.state));
  return {
    root,
    members,
    current,
    completedBytes: size.completedBytes,
    totalBytes: size.totalBytes,
    percent:
      size.totalBytes > 0
        ? Math.min(100, (size.completedBytes / size.totalBytes) * 100)
        : 0,
    active,
    attention,
    phase: current ? phaseLabel(current, current.state) : root.label,
  };
}

/** Return the aggregate size even when a cancelled partial install has no active group. */
export function modelDownloadSize(
  root: ModelDescriptor,
  models: ModelDescriptor[],
): ModelDownloadSize {
  const byId = new Map(models.map((model) => [model.id, model]));
  return aggregateSize(dependencyClosure(root, byId));
}

export function visibleModelDependencies(
  model: ModelDescriptor,
  dependencies: ResolvedModelDependency[],
): ResolvedModelDependency[] {
  return dependencies.filter(({ model: dependency, required }) =>
    dependency.state === "queued" ||
    dependency.state === "downloading" ||
    dependency.state === "paused" ||
    dependency.state === "verifying" ||
    dependency.state === "failed" ||
    dependency.state === "corrupt" ||
    (required && model.state === "installed" && dependency.state !== "installed"),
  );
}
