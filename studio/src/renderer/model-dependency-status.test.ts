import { describe, expect, it } from "vitest";
import type { ModelDescriptor } from "./platform/desktop";
import {
  formatDownloadSize,
  formatDownloadSizePair,
  modelDownloadGroup,
  modelDownloadSize,
  visibleModelDependencies,
} from "./model-dependency-status";

function model(
  id: string,
  state: ModelDescriptor["state"],
  overrides: Partial<ModelDescriptor> = {},
): ModelDescriptor {
  return {
    id,
    label: id,
    kind: id === "aligner" ? "aligner" : "asr",
    ui_role: id === "aligner" ? "dependency" : "primary",
    download_source: "modelscope",
    revision: "test",
    size_bytes: 1,
    memory_bytes: 1,
    license: "Apache-2.0",
    dependencies: [],
    state,
    downloaded_bytes: 0,
    ...overrides,
  };
}

describe("model dependency display", () => {
  it("formats download sizes consistently across byte units", () => {
    expect(formatDownloadSize(0)).toBe("0 B");
    expect(formatDownloadSize(512)).toBe("512 B");
    expect(formatDownloadSize(1536)).toBe("2 KB");
    expect(formatDownloadSize(1.5 * 1024 ** 2)).toBe("1.5 MB");
    expect(formatDownloadSize(2 * 1024 ** 3)).toBe("2.0 GB");
    expect(formatDownloadSizePair(742 * 1024 ** 2, 2 * 1024 ** 3)).toBe(
      "742 MB / 2.0 GB",
    );
  });

  it("only exposes an inline dependency while it needs attention", () => {
    const asr = model("asr", "not_installed");
    const aligner = model("aligner", "not_installed");
    const dependency = {
      model_id: aligner.id,
      required: true,
      reason: "逐词时间锚点",
      model: aligner,
    };

    expect(visibleModelDependencies(asr, [dependency])).toEqual([]);
    expect(
      visibleModelDependencies(asr, [
        { ...dependency, model: { ...aligner, state: "downloading" } },
      ]),
    ).toHaveLength(1);
    expect(
      visibleModelDependencies({ ...asr, state: "installed" }, [dependency]),
    ).toHaveLength(1);
  });

  it("combines a queued ASR and downloading ForcedAligner by exact bytes", () => {
    const aligner = model("aligner", "downloading", {
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      size_bytes: 200,
      downloaded_bytes: 50,
    });
    const asr = model("asr", "queued", {
      label: "Qwen3 ASR 0.6B · 4-bit",
      size_bytes: 800,
      dependencies: [{ model_id: aligner.id, required: true }],
    });

    const group = modelDownloadGroup(asr, [asr, aligner]);

    expect(group).toMatchObject({
      totalBytes: 1000,
      completedBytes: 50,
      percent: 5,
      phase: "正在下载 ForcedAligner",
      active: true,
    });
    expect(group?.current?.label).toBe("Qwen3 ForcedAligner 0.6B · 8-bit");
    expect(group?.members.map((item) => item.id)).toEqual(["asr", "aligner"]);
    expect(formatDownloadSizePair(group!.completedBytes, group!.totalBytes)).toBe(
      "50 B / 1000 B",
    );
  });

  it("keeps the weighted progress monotonic when the main ASR starts", () => {
    const aligner = model("aligner", "installed", {
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      size_bytes: 200,
      downloaded_bytes: 200,
    });
    const asr = model("asr", "downloading", {
      label: "Qwen3 ASR 0.6B · 4-bit",
      size_bytes: 800,
      downloaded_bytes: 100,
      dependencies: [{ model_id: aligner.id, required: true }],
    });

    const group = modelDownloadGroup(asr, [asr, aligner]);

    expect(group?.completedBytes).toBe(300);
    expect(group?.percent).toBe(30);
    expect(group?.phase).toBe("正在下载 Qwen3 ASR 0.6B · 4-bit");
    expect(modelDownloadSize(asr, [asr, aligner])).toEqual({
      completedBytes: 300,
      totalBytes: 1000,
    });
  });

  it("does not attach a manually downloaded dependency to an idle primary", () => {
    const aligner = model("aligner", "downloading", {
      kind: "aligner",
      size_bytes: 200,
      downloaded_bytes: 50,
    });
    const asr = model("asr", "not_installed", {
      size_bytes: 800,
      dependencies: [{ model_id: aligner.id, required: true }],
    });

    expect(modelDownloadGroup(asr, [asr, aligner])).toBeNull();
  });

  it("uses the same group calculation for a speaker model and VAD", () => {
    const vad = model("vad", "downloading", {
      label: "Silero VAD v6 · MLX",
      kind: "vad",
      size_bytes: 100,
      downloaded_bytes: 25,
    });
    const speaker = model("speaker", "queued", {
      label: "说话人识别 · MLX",
      kind: "speaker",
      size_bytes: 300,
      dependencies: [{ model_id: vad.id, required: true }],
    });

    const group = modelDownloadGroup(speaker, [speaker, vad]);

    expect(group?.totalBytes).toBe(400);
    expect(group?.completedBytes).toBe(25);
    expect(group?.phase).toBe("正在下载 VAD");
  });
});
