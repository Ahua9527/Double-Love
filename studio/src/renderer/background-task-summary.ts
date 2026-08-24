import type { ModelDescriptor, ModelInstallState } from "./platform/desktop";
import type { ModelQueueSnapshot } from "../../../bindings/ModelQueueSnapshot";
import {
  modelDownloadGroup,
  type ModelDownloadGroup,
} from "./model-dependency-status";

const ACTIVE_MODEL_STATES: ModelInstallState[] = [
  "queued",
  "downloading",
  "verifying",
];
const ATTENTION_MODEL_STATES: ModelInstallState[] = [
  "paused",
  "failed",
  "corrupt",
];

export interface RuntimeBackgroundTask {
  kind: "transcribe" | "speaker";
  message: string;
}

export interface BackgroundTaskSummary {
  count: number;
  tone: "active" | "attention";
  activeGroups: ModelDownloadGroup[];
  attentionGroups: ModelDownloadGroup[];
}

function groupedModels(
  models: ModelDescriptor[],
  states: ModelInstallState[],
  attention: boolean,
): ModelDownloadGroup[] {
  const consumed = new Set<string>();
  const groups: ModelDownloadGroup[] = [];
  const add = (group: ModelDownloadGroup | null) => {
    if (!group) return;
    const relevant = attention ? group.attention : group.active;
    if (!relevant || group.members.some((model) => consumed.has(model.id))) return;
    groups.push(group);
    group.members.forEach((model) => consumed.add(model.id));
  };

  // Prefer a primary model as the group root. This collapses a queued ASR and
  // its downloading ForcedAligner into one background task.
  models
    .filter((model) => model.ui_role === "primary")
    .forEach((model) => add(modelDownloadGroup(model, models)));

  // A dependency installed or repaired by itself remains visible as a task.
  models.forEach((model) => {
    if (consumed.has(model.id) || !states.includes(model.state)) return;
    add(modelDownloadGroup(model, models));
  });
  return groups;
}

export function summarizeBackgroundTasks(
  task: RuntimeBackgroundTask | null,
  models: ModelDescriptor[],
  queue: ModelQueueSnapshot = { active_model_id: null, entries: [] },
): BackgroundTaskSummary {
  const activeGroups = groupedModels(models, ACTIVE_MODEL_STATES, false);
  for (const entry of queue.entries) {
    if (activeGroups.some((group) => group.root.id === entry.model_id)) continue;
    const root = models.find((model) => model.id === entry.model_id);
    const group = root ? modelDownloadGroup(root, models) : null;
    if (group) activeGroups.push(group);
  }
  const attentionGroups = groupedModels(models, ATTENTION_MODEL_STATES, true);
  return {
    count: (task ? 1 : 0) + activeGroups.length + attentionGroups.length,
    tone: task || activeGroups.length > 0 ? "active" : "attention",
    activeGroups,
    attentionGroups,
  };
}
