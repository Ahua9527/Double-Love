import {
  Activity,
  AlertTriangle,
  AudioLines,
  CheckCircle2,
} from "lucide-react";
import {
  summarizeBackgroundTasks,
  type RuntimeBackgroundTask,
} from "../background-task-summary";
import {
  formatDownloadSizePair,
  type ModelDownloadGroup,
} from "../model-dependency-status";
import type { ModelDescriptor } from "../platform/desktop";
import type { ModelQueueEntry } from "../../../../bindings/ModelQueueEntry";
import type { ModelQueueSnapshot } from "../../../../bindings/ModelQueueSnapshot";

function ModelTaskRow({
  group,
  queueEntry,
  attention,
  cancelling,
  onCancelModel,
  onOpenModels,
}: {
  group: ModelDownloadGroup;
  queueEntry?: ModelQueueEntry;
  attention: boolean;
  cancelling: boolean;
  onCancelModel: (modelId: string) => void;
  onOpenModels: () => void;
}) {
  const progress = Math.min(100, group.percent);
  const currentLabel = group.current?.label;
  const phase = queueEntry?.state === "queued"
    ? `队列第 ${queueEntry.position} 位`
    : currentLabel &&
    (group.phase === "正在下载 ForcedAligner" || group.phase === "正在下载 VAD")
      ? `${group.phase} · ${currentLabel}`
      : group.phase;
  const error = group.members.find((model) => model.error)?.error;
  return (
    <article className={`studio-task-row${attention ? " is-attention" : ""}`}>
      <span className="studio-task-icon" aria-hidden="true">
        {attention ? <AlertTriangle size={16} /> : <AudioLines size={16} />}
      </span>
      <div className="studio-task-copy">
        <strong>{group.root.label}</strong>
        <p className="studio-task-phase">{error ? `${phase} · ${error}` : phase}</p>
        <span className="studio-task-size">
          {formatDownloadSizePair(group.completedBytes, group.totalBytes)}
        </span>
        {!attention && (
          <div
            className="studio-task-progress"
            role="progressbar"
            aria-label={`${group.root.label}整体下载进度`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(progress)}
            aria-valuetext={`${Math.round(progress)}% · ${formatDownloadSizePair(group.completedBytes, group.totalBytes)} · ${phase}`}
          >
            <i style={{ width: `${progress}%` }} />
          </div>
        )}
      </div>
      <div className="studio-task-actions">
        <button
          type="button"
          className="studio-secondary-button"
          onClick={onOpenModels}
        >
          查看
        </button>
        <button
          type="button"
          className="studio-secondary-button"
          disabled={cancelling}
          onClick={() => onCancelModel(group.root.id)}
        >
          {cancelling ? "处理中…" : attention ? "清除任务" : "取消下载"}
        </button>
      </div>
    </article>
  );
}

interface BackgroundTasksProps {
  task: RuntimeBackgroundTask | null;
  models: ModelDescriptor[];
  queue?: ModelQueueSnapshot;
  onCancelTask: () => void;
  onCancelModel: (modelId: string) => void;
  onOpenModels: () => void;
  cancellingModelId?: string | null;
}

export function BackgroundTasks({
  task,
  models,
  queue = { active_model_id: null, entries: [] },
  onCancelTask,
  onCancelModel,
  onOpenModels,
  cancellingModelId = null,
}: BackgroundTasksProps) {
  const summary = summarizeBackgroundTasks(task, models, queue);
  const hasActive = Boolean(task) || summary.activeGroups.length > 0;
  const hasAttention = summary.attentionGroups.length > 0;

  return (
    <section className="studio-tasks" aria-labelledby="background-tasks-title">
      <header>
        <div>
          <h1 id="background-tasks-title">后台任务</h1>
          <p>下载、转录与说话人处理都在本机运行。</p>
        </div>
      </header>
      {!hasActive && !hasAttention ? (
        <div className="studio-tasks-empty">
          <CheckCircle2 size={22} />
          <strong>当前没有后台任务</strong>
          <p>没有正在运行或需要处理的项目。</p>
        </div>
      ) : (
        <div className="studio-task-sections">
          {hasActive && (
            <section aria-labelledby="active-tasks-title">
              <h2 id="active-tasks-title">进行中</h2>
              {task && (
                <article className="studio-task-row">
                  <span className="studio-task-icon" aria-hidden="true">
                    <Activity size={16} />
                  </span>
                  <div className="studio-task-copy">
                    <strong>
                      {task.kind === "speaker" ? "说话人分离" : "转录"}
                    </strong>
                    <p>{task.message}</p>
                  </div>
                  <button
                    type="button"
                    className="studio-secondary-button"
                    onClick={onCancelTask}
                  >
                    取消
                  </button>
                </article>
              )}
              {summary.activeGroups.map((group) => (
                <ModelTaskRow
                  key={group.root.id}
                  group={group}
                  queueEntry={queue.entries.find((entry) => entry.model_id === group.root.id)}
                  attention={false}
                  cancelling={cancellingModelId === group.root.id}
                  onCancelModel={onCancelModel}
                  onOpenModels={onOpenModels}
                />
              ))}
            </section>
          )}
          {hasAttention && (
            <section aria-labelledby="attention-tasks-title">
              <h2 id="attention-tasks-title">需要处理</h2>
              {summary.attentionGroups.map((group) => (
                <ModelTaskRow
                  key={group.root.id}
                  group={group}
                  queueEntry={queue.entries.find((entry) => entry.model_id === group.root.id)}
                  attention
                  cancelling={cancellingModelId === group.root.id}
                  onCancelModel={onCancelModel}
                  onOpenModels={onOpenModels}
                />
              ))}
            </section>
          )}
        </div>
      )}
    </section>
  );
}
