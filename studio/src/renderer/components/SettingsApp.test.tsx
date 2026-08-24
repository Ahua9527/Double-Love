import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelDescriptor } from "../platform/desktop";
import { LegacyCleanupDialog, ModelsPage, SettingsApp } from "./SettingsApp";

describe("SettingsApp", () => {
  it("在浏览器预览中展示七页设置并明确未连接桌面服务", () => {
    render(<SettingsApp />);
    expect(screen.getByText(/浏览器预览：读取和操作模型/)).toBeTruthy();
    expect(screen.getByRole("heading", { name: "通用" })).toBeTruthy();
    expect((screen.getByLabelText("回滚上限") as HTMLSelectElement).value).toBe("200");
    expect(screen.getByRole("button", { name: "本地模型" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "诊断" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "本地模型" }));
    expect(screen.getByRole("heading", { name: "本地模型" })).toBeTruthy();
    expect(screen.getAllByText("Qwen3 ASR 0.6B · 4-bit").length).toBeGreaterThan(0);
    expect(screen.queryByText("ModelScope")).toBeNull();
    expect(screen.getByText("说话人识别 · MLX")).toBeTruthy();
    expect(screen.getByText("约 26 MB · 本地说话人区分 · MLX")).toBeTruthy();
    expect(screen.queryByText("依赖组件")).toBeNull();
    expect(screen.queryByText(/Forced Aligner/)).toBeNull();
    expect(screen.queryByText(/Silero VAD/)).toBeNull();
    expect(screen.queryByLabelText("默认转录模型")).toBeNull();
    expect(screen.queryByText(/推荐配置/)).toBeNull();
    expect(screen.getAllByText("推荐")).toHaveLength(1);
    expect(screen.getByText("2 GB RAM • ~10x realtime • High accuracy")).toBeTruthy();
    expect(screen.getByText("5 GB RAM • ~5x realtime • Highest accuracy")).toBeTruthy();
    expect(screen.queryByText(/revision 7278e1e/)).toBeNull();
    expect(screen.queryByLabelText("模型下载源")).toBeNull();
    expect(screen.queryByText("下载来源策略")).toBeNull();
    expect(screen.queryByText("模型目录")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "通用" }));
    expect(screen.queryByLabelText("时间码精度")).toBeNull();
  });

  it("浏览器预览的模型操作不会伪装成成功", () => {
    render(<SettingsApp initialPage="models" />);
    fireEvent.click(screen.getAllByRole("button", { name: /^安装/ })[0]);
    expect(
      screen.getByText("浏览器预览：模型操作需要在桌面应用中执行。"),
    ).toBeTruthy();
  });

  it("未安装模型支持右键和键盘打开本地导入菜单", () => {
    render(<SettingsApp initialPage="models" />);
    const row = screen.getByText("Qwen3 ASR 0.6B · 4-bit").closest("article")!;

    fireEvent.contextMenu(row, { clientX: 40, clientY: 50 });
    expect(screen.getByRole("menu", { name: /Qwen3 ASR 0.6B · 4-bit/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "从文件夹导入…" })).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.keyDown(row, { key: "F10", shiftKey: true });
    expect(screen.getByRole("menuitem", { name: "从文件夹导入…" })).toBeTruthy();
  });

  it("已安装模型可以在文件管理器中显示，缺失的内联依赖可以从文件夹导入", () => {
    const dependency: ModelDescriptor = {
      id: "aligner",
      label: "共享时间戳模型",
      kind: "forced_aligner",
      ui_role: "dependency",
      download_source: "modelscope",
      revision: "test",
      size_bytes: 5,
      memory_bytes: 0,
      license: "Apache-2.0",
      dependencies: [],
      state: "not_installed",
      installed_revision: null,
      downloaded_bytes: 0,
      can_remove: false,
    };
    const model: ModelDescriptor = {
      ...dependency,
      id: "asr",
      label: "本地 ASR",
      kind: "asr",
      ui_role: "primary",
      dependencies: [
        { model_id: dependency.id, required: true, reason: "逐词时间戳" },
      ],
      replaces_model_id: "old-asr",
      state: "installed",
      installed_revision: "test",
      can_remove: true,
    };
    const onImport = vi.fn();
    const onReveal = vi.fn();
    const onCleanup = vi.fn();
    render(
      <ModelsPage
        models={[model, dependency]}
        modelsLoading={false}
        modelBusy={null}
        recommended="asr"
        onAction={vi.fn()}
        onImport={onImport}
        onReveal={onReveal}
        onCleanup={onCleanup}
      />,
    );

    fireEvent.contextMenu(screen.getByText("本地 ASR").closest("article")!);
    fireEvent.click(screen.getByRole("menuitem", { name: /访达|所在文件夹/ }));
    expect(onReveal).toHaveBeenCalledWith(model);

    fireEvent.contextMenu(screen.getByText("本地 ASR").closest("article")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "清理旧版本…" }));
    expect(onCleanup).toHaveBeenCalledWith(model);

    fireEvent.contextMenu(screen.getByText(/共享时间戳模型 · 未安装/));
    fireEvent.click(screen.getByRole("menuitem", { name: "从文件夹导入…" }));
    expect(onImport).toHaveBeenCalledWith(dependency);
  });

  it("把 Qwen 与 ForcedAligner 显示为一条加权总进度", () => {
    const aligner: ModelDescriptor = {
      id: "aligner",
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      ui_role: "dependency",
      download_source: "modelscope",
      revision: "test",
      size_bytes: 200,
      memory_bytes: 0,
      license: "Apache-2.0",
      dependencies: [],
      state: "downloading",
      downloaded_bytes: 50,
      can_remove: false,
    };
    const asr: ModelDescriptor = {
      id: "asr",
      label: "Qwen3 ASR 0.6B · 4-bit",
      kind: "asr",
      ui_role: "primary",
      download_source: "modelscope",
      revision: "test",
      size_bytes: 800,
      memory_bytes: 0,
      license: "Apache-2.0",
      dependencies: [
        {
          model_id: aligner.id,
          required: true,
          reason: "Qwen3 ASR 逐词时间戳需要共享 ForcedAligner",
        },
      ],
      state: "queued",
      downloaded_bytes: 0,
    };

    render(
      <ModelsPage
        models={[asr, aligner]}
        modelsLoading={false}
        modelBusy={null}
        recommended="asr"
        onAction={vi.fn()}
        onImport={vi.fn()}
        onReveal={vi.fn()}
        onCleanup={vi.fn()}
      />,
    );

    expect(screen.getByText("正在下载 ForcedAligner")).toBeTruthy();
    expect(screen.getAllByText("Qwen3 ForcedAligner 0.6B · 8-bit").length).toBeGreaterThan(0);
    expect(screen.getByText("50 B / 1000 B")).toBeTruthy();
    expect(screen.getByText("安装中")).toBeTruthy();
    expect(screen.getAllByRole("progressbar")).toHaveLength(1);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("5");
    expect(screen.queryByText(/需要共享 ForcedAligner/)).toBeNull();
  });

  it("取消后隐藏进度条但保留可续传提示", () => {
    const aligner: ModelDescriptor = {
      id: "aligner",
      label: "Qwen3 ForcedAligner 0.6B · 8-bit",
      kind: "aligner",
      ui_role: "dependency",
      download_source: "modelscope",
      revision: "test",
      size_bytes: 200,
      memory_bytes: 0,
      license: "Apache-2.0",
      dependencies: [],
      state: "installed",
      downloaded_bytes: 200,
      can_remove: false,
    };
    const asr: ModelDescriptor = {
      id: "asr",
      label: "Qwen3 ASR 0.6B · 4-bit",
      kind: "asr",
      ui_role: "primary",
      download_source: "modelscope",
      revision: "test",
      size_bytes: 800,
      memory_bytes: 0,
      license: "Apache-2.0",
      dependencies: [{ model_id: aligner.id, required: true }],
      state: "not_installed",
      downloaded_bytes: 100,
    };

    render(
      <ModelsPage
        models={[asr, aligner]}
        modelsLoading={false}
        modelBusy={null}
        recommended="asr"
        onAction={vi.fn()}
        onImport={vi.fn()}
        onReveal={vi.fn()}
        onCleanup={vi.fn()}
      />,
    );

    expect(screen.getByText("已下载 300 B / 1000 B，可继续安装")).toBeTruthy();
    expect(screen.queryAllByRole("progressbar")).toHaveLength(0);
  });

  it("旧模型只显示占用与升级提示，不提供安装或导入操作", () => {
    const current: ModelDescriptor = {
      id: "current-asr",
      label: "当前 ASR",
      kind: "asr",
      ui_role: "primary",
      download_source: "modelscope",
      revision: "current",
      size_bytes: 1,
      memory_bytes: 0,
      license: "MIT",
      dependencies: [],
      state: "not_installed",
      replaces_model_id: "old-asr",
    };
    const legacy: ModelDescriptor = {
      ...current,
      id: "old-asr",
      label: "旧版 ASR",
      ui_role: "legacy",
      state: "installed",
      size_bytes: 64,
    };
    render(
      <ModelsPage
        models={[current, legacy]}
        modelsLoading={false}
        modelBusy={null}
        recommended="current-asr"
        onAction={vi.fn()}
        onImport={vi.fn()}
        onReveal={vi.fn()}
        onCleanup={vi.fn()}
      />,
    );
    expect(screen.getByText("旧模型需要升级")).toBeTruthy();
    expect(screen.getByText("旧版 ASR")).toBeTruthy();
    expect(screen.queryByLabelText(/安装旧版 ASR/)).toBeNull();
  });

  it("旧版本清理确认清楚说明可释放空间和被保留的共享依赖", () => {
    const model: ModelDescriptor = {
      id: "current-asr",
      label: "当前 ASR",
      kind: "asr",
      ui_role: "primary",
      download_source: "modelscope",
      revision: "current",
      size_bytes: 1,
      memory_bytes: 0,
      license: "MIT",
      dependencies: [],
      state: "installed",
      replaces_model_id: "old-asr",
    };
    const onConfirm = vi.fn();
    render(
      <LegacyCleanupDialog
        model={model}
        preview={{
          target_model_id: model.id,
          bytes_to_free: 1024,
          removable: [
            {
              model_id: "old-asr",
              display_name: "旧版 ASR",
              bytes_to_free: 1024,
              reason: null,
            },
          ],
          retained: [
            {
              model_id: "old-aligner",
              display_name: "旧版对齐器",
              bytes_to_free: 0,
              reason: "仍由 old-asr-b 使用",
            },
          ],
        }}
        busy={false}
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );
    expect(screen.getByText("清理旧版本？")).toBeTruthy();
    expect(screen.getByText("旧版 ASR")).toBeTruthy();
    expect(screen.getByText(/旧版对齐器：仍由 old-asr-b 使用/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "清理旧版本" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("关于页显示 0.2.0，并在满足状态前禁用下载与安装", async () => {
    render(<SettingsApp initialPage="about" />);
    expect(await screen.findByText("版本 0.2.0")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "检查更新" }).hasAttribute("disabled"),
    ).toBe(false);
    expect(
      screen.getByRole("button", { name: "下载更新" }).hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen.getByRole("button", { name: "重启安装" }).hasAttribute("disabled"),
    ).toBe(true);
  });
});
