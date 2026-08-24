import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelDescriptor, ModelInstallState } from "../platform/desktop";
import { summarizeBackgroundTasks } from "../background-task-summary";
import { BackgroundTasks } from "./BackgroundTasks";

function model(
  id: string,
  state: ModelInstallState,
  overrides: Partial<ModelDescriptor> = {},
): ModelDescriptor {
  return {
    id,
    label: id,
    kind: "asr",
    ui_role: "primary",
    download_source: "modelscope",
    revision: "test",
    size_bytes: 100,
    memory_bytes: 0,
    license: "Apache-2.0",
    dependencies: [],
    state,
    downloaded_bytes: 50,
    ...overrides,
  };
}

describe("background task summary", () => {
  it("hides the badge when every item is complete or cancelled", () => {
    expect(
      summarizeBackgroundTasks(null, [
        model("installed", "installed"),
        model("idle", "not_installed"),
      ]),
    ).toMatchObject({ count: 0 });
  });

  it("uses blue while anything is active and counts all actionable states", () => {
    const summary = summarizeBackgroundTasks(
      { kind: "transcribe", message: "运行中" },
      [
        model("queued", "queued"),
        model("downloading", "downloading"),
        model("verifying", "verifying"),
        model("paused", "paused"),
        model("failed", "failed"),
        model("corrupt", "corrupt"),
      ],
    );
    expect(summary.count).toBe(7);
    expect(summary.tone).toBe("active");
  });

  it("uses amber when only paused or failed items remain", () => {
    expect(
      summarizeBackgroundTasks(null, [
        model("paused", "paused"),
        model("failed", "failed"),
      ]),
    ).toMatchObject({ count: 2, tone: "attention" });
  });

  it("collapses a queued ASR and its active ForcedAligner into one task", () => {
    const aligner = model("aligner", "downloading", {
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      size_bytes: 200,
      downloaded_bytes: 50,
    });
    const asr = model("asr", "queued", {
      label: "Qwen3 ASR 0.6B · 4-bit",
      size_bytes: 800,
      downloaded_bytes: 0,
      dependencies: [{ model_id: aligner.id, required: true }],
    });

    const summary = summarizeBackgroundTasks(null, [asr, aligner]);

    expect(summary.count).toBe(1);
    expect(summary.activeGroups).toHaveLength(1);
    expect(summary.activeGroups[0]?.root.id).toBe("asr");
  });
});

describe("BackgroundTasks", () => {
  it("separates active and attention items without completed history", () => {
    const onOpenModels = vi.fn();
    const onCancelModel = vi.fn();
    render(
      <BackgroundTasks
        task={null}
        models={[
          model("download", "downloading"),
          model("broken", "corrupt"),
          model("done", "installed"),
        ]}
        onCancelTask={vi.fn()}
        onCancelModel={onCancelModel}
        onOpenModels={onOpenModels}
      />,
    );
    expect(screen.getByRole("heading", { name: "进行中" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "需要处理" })).toBeTruthy();
    expect(screen.queryByText("done")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "清除任务" }));
    expect(onCancelModel).toHaveBeenCalledWith("broken");
    fireEvent.click(screen.getAllByRole("button", { name: "查看" })[0]!);
    expect(onOpenModels).toHaveBeenCalledOnce();
  });

  it("returns to an explicit empty state", () => {
    render(
      <BackgroundTasks
        task={null}
        models={[]}
        onCancelTask={vi.fn()}
        onCancelModel={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    expect(screen.getByText("当前没有后台任务")).toBeTruthy();
  });

  it("shows the combined progress and current dependency phase", () => {
    const aligner = model("aligner", "downloading", {
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      size_bytes: 200,
      downloaded_bytes: 50,
    });
    const asr = model("asr", "queued", {
      label: "Qwen3 ASR 0.6B · 4-bit",
      size_bytes: 800,
      downloaded_bytes: 0,
      dependencies: [{ model_id: aligner.id, required: true }],
    });

    const onCancelModel = vi.fn();
    render(
      <BackgroundTasks
        task={null}
        models={[asr, aligner]}
        onCancelTask={vi.fn()}
        onCancelModel={onCancelModel}
        onOpenModels={vi.fn()}
      />,
    );

    expect(screen.getByText("正在下载 ForcedAligner · Qwen3 ForcedAligner 0.6B · 8-bit")).toBeTruthy();
    expect(screen.getByText("50 B / 1000 B")).toBeTruthy();
    expect(screen.getAllByRole("progressbar")).toHaveLength(1);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("5");
    fireEvent.click(screen.getByRole("button", { name: "取消下载" }));
    expect(onCancelModel).toHaveBeenCalledWith("asr");
  });

  it("keeps two Qwen requests separate and shows the second queue position", () => {
    const aligner = model("aligner", "downloading", {
      kind: "aligner",
      size_bytes: 200,
      dependencies: [],
    });
    const first = model("qwen-small", "queued", {
      label: "Qwen3 ASR 0.6B · 4-bit",
      dependencies: [{ model_id: aligner.id, required: true }],
    });
    const second = model("qwen-large", "queued", {
      label: "Qwen3 ASR 1.7B · 8-bit",
      dependencies: [{ model_id: aligner.id, required: true }],
    });
    render(
      <BackgroundTasks
        task={null}
        models={[first, second, aligner]}
        queue={{
          active_model_id: "qwen-small",
          entries: [
            { model_id: "qwen-small", position: 1, state: "active" },
            { model_id: "qwen-large", position: 2, state: "queued" },
          ],
        }}
        onCancelTask={vi.fn()}
        onCancelModel={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    expect(screen.getByText("Qwen3 ASR 0.6B · 4-bit")).toBeTruthy();
    expect(screen.getByText("Qwen3 ASR 1.7B · 8-bit")).toBeTruthy();
    expect(screen.getByText("队列第 2 位")).toBeTruthy();
    expect(screen.getAllByRole("progressbar")).toHaveLength(2);
  });
});
