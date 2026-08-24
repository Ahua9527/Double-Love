import { afterEach, describe, expect, it, vi } from "vitest";
import {
  assetsList,
  createProject,
  getAppInfo,
  importMedia,
  listen,
  modelLegacyCleanupApply,
  modelLegacyCleanupPreview,
  modelImportFolder,
  modelReveal,
  pickDirectory,
  recentProjectOpen,
  updateCheck,
} from "./electron";

function installBridge(
  invoke: (name: string, payload?: unknown) => Promise<unknown>,
  onEvent = vi.fn(),
) {
  Object.defineProperty(window, "doubleLove", {
    configurable: true,
    value: {
      hostHealth: vi.fn(),
      openSettings: vi.fn(),
      getAppInfo: vi
        .fn()
        .mockResolvedValue({ name: "Double Love Studio", version: "0.2.0" }),
      createProject: vi.fn(),
      updates: {
        check: vi.fn().mockResolvedValue({ stage: "update-not-available" }),
        download: vi.fn(),
        install: vi.fn(),
      },
      dialogs: {
        pickDirectory: vi
          .fn()
          .mockResolvedValue({
            token: "directory-grant",
            displayName: "Projects",
          }),
        pickMediaFile: vi.fn().mockResolvedValue(null),
        pickExportPath: vi.fn().mockResolvedValue(null),
      },
      invoke,
      onEvent,
    },
  });
  return window as unknown as {
    doubleLove: {
      createProject: ReturnType<typeof vi.fn>;
      dialogs: { pickDirectory: ReturnType<typeof vi.fn> };
    };
  };
}

afterEach(() => {
  Reflect.deleteProperty(window, "doubleLove");
});

describe("Electron renderer adapter", () => {
  it("unwraps invoke HostResponse data and preserves command payloads", async () => {
    const operation = {
      status: "success",
      revision: null,
      data: [],
      counts: { total: 0, processed: 0, skipped: 0, failed: 0, unmatched: 0 },
      diagnostics: [],
      outputs: [],
    };
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: "request-1",
      status: "ok",
      result: { type: "invoke", data: operation },
    });
    installBridge(invoke);

    await expect(assetsList()).resolves.toBe(operation);
    expect(invoke).toHaveBeenCalledWith("assets_list", undefined);
  });

  it("maps HostResponse errors to failed OperationResult diagnostics", async () => {
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: "request-2",
      status: "error",
      error: {
        code: "UNKNOWN_COMMAND",
        message: "unknown command: import_media",
      },
    });
    installBridge(invoke);

    const result = await importMedia("opaque-token");
    expect(invoke).toHaveBeenCalledWith("import_media", {
      grantToken: "opaque-token",
    });
    expect(result).toMatchObject({
      status: "failed",
      data: null,
      counts: { total: 0, processed: 0, skipped: 0, failed: 1, unmatched: 0 },
      diagnostics: [
        {
          code: "UNKNOWN_COMMAND",
          cause: "unknown command: import_media",
          impact: "操作未产生可用结果",
          blocks_export: true,
        },
      ],
    });
  });

  it("returns opaque dialog tokens and forwards the grant discriminator", async () => {
    const installed = installBridge(vi.fn());
    await expect(pickDirectory("选择已有项目", "project-open")).resolves.toBe(
      "directory-grant",
    );
    await expect(pickDirectory("缺少授权类型")).rejects.toThrow(
      "require a grant kind",
    );
    expect(installed.doubleLove.dialogs.pickDirectory).toHaveBeenCalledWith({
      title: "选择已有项目",
      kind: "project-open",
    });
  });

  it("passes only a model id, one-time grant, and license confirmation", async () => {
    const operation = {
      status: "success",
      revision: null,
      data: null,
      counts: { total: 0, processed: 0, skipped: 0, failed: 0, unmatched: 0 },
      diagnostics: [],
      outputs: [],
    };
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: "model-1",
      status: "ok",
      result: { type: "invoke", data: operation },
    });
    installBridge(invoke);

    await modelImportFolder(
      "wespeaker-voxceleb-resnet34-lm",
      "opaque-model-grant",
    );
    await modelReveal("wespeaker-voxceleb-resnet34-lm");
    expect(invoke).toHaveBeenNthCalledWith(1, "model_import_folder", {
      modelId: "wespeaker-voxceleb-resnet34-lm",
      grantToken: "opaque-model-grant",
      acceptNoncommercialLicense: false,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "model_reveal", {
      modelId: "wespeaker-voxceleb-resnet34-lm",
    });
    expect(JSON.stringify(invoke.mock.calls)).not.toContain("/Users/");
  });

  it("requests legacy cleanup only by current model id and explicit confirmation", async () => {
    const operation = {
      status: "success",
      revision: null,
      data: {
        target_model_id: "qwen3-asr-0.6b-4bit",
        bytes_to_free: 5,
        removable: [],
        retained: [],
      },
      counts: { total: 0, processed: 0, skipped: 0, failed: 0, unmatched: 0 },
      diagnostics: [],
      outputs: [],
    };
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: "cleanup-1",
      status: "ok",
      result: { type: "invoke", data: operation },
    });
    installBridge(invoke);

    await modelLegacyCleanupPreview("qwen3-asr-0.6b-4bit");
    await modelLegacyCleanupApply("qwen3-asr-0.6b-4bit");
    expect(invoke).toHaveBeenNthCalledWith(1, "model_legacy_cleanup_preview", {
      modelId: "qwen3-asr-0.6b-4bit",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "model_legacy_cleanup_apply", {
      modelId: "qwen3-asr-0.6b-4bit",
      confirmed: true,
    });
    expect(JSON.stringify(invoke.mock.calls)).not.toContain("/Users/");
  });

  it("creates named projects without sending an absolute path", async () => {
    const installed = installBridge(vi.fn());
    installed.doubleLove.createProject.mockResolvedValue({
      v: 1,
      id: "create-1",
      status: "ok",
      result: {
        type: "invoke",
        data: {
          status: "success",
          revision: null,
          data: {
            project_id: "project-1",
            root: "/safe/root",
            database: "/safe/root/.doublelove/project.sqlite",
            revision: 0,
          },
          counts: {
            total: 0,
            processed: 0,
            skipped: 0,
            failed: 0,
            unmatched: 0,
          },
          diagnostics: [],
          outputs: [],
        },
      },
    });

    const result = await createProject({ name: "春日采访" });
    expect(result.status).toBe("success");
    expect(installed.doubleLove.createProject).toHaveBeenCalledWith({
      name: "春日采访",
    });
    expect(
      JSON.stringify(installed.doubleLove.createProject.mock.calls[0]),
    ).not.toContain("/Users/");
  });

  it("opens registered projects by project id", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValue({
        status: "error",
        error: {
          code: "RECENT_PROJECT_MISSING",
          message: "项目位置已经丢失。",
        },
      });
    installBridge(invoke);
    const result = await recentProjectOpen("project-1");
    expect(invoke).toHaveBeenCalledWith("recent_project_open", {
      projectId: "project-1",
    });
    expect(result.diagnostics[0]?.code).toBe("RECENT_PROJECT_MISSING");
  });

  it("reads app metadata and updater state only through the preload bridge", async () => {
    installBridge(vi.fn());
    await expect(getAppInfo()).resolves.toEqual({
      name: "Double Love Studio",
      version: "0.2.0",
    });
    await expect(updateCheck()).resolves.toEqual({
      stage: "update-not-available",
    });
  });

  it("maps event payloads and exposes the preload unsubscribe function", async () => {
    const unsubscribe = vi.fn();
    let hostCallback: ((payload: unknown) => void) | undefined;
    const onEvent = vi.fn(
      (_channel: string, callback: (payload: unknown) => void) => {
        hostCallback = callback;
        return unsubscribe;
      },
    );
    installBridge(vi.fn(), onEvent);
    const callback = vi.fn();

    const remove = await listen<{ completed: number }>(
      "dl://progress",
      callback,
    );
    hostCallback?.({ completed: 3 });
    expect(callback).toHaveBeenCalledWith({ payload: { completed: 3 } });
    remove();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});
