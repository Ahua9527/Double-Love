import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import {
  AlertTriangle,
  AudioLines,
  Check,
  ChevronRight,
  ClipboardCheck,
  Download,
  FolderOpen,
  Info,
  Keyboard,
  LockKeyhole,
  Pause,
  Play,
  RefreshCw,
  Settings2,
  SlidersHorizontal,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import * as api from "../platform/desktop";
import type { SubtitleStyle } from "../../../../bindings/SubtitleStyle";
import { num } from "../utils";
import {
  formatDownloadSizePair,
  modelDownloadGroup,
  modelDownloadSize,
  visibleModelDependencies,
  type ModelDownloadGroup,
  type ModelDownloadSize,
  type ResolvedModelDependency,
} from "../model-dependency-status";
import { Tooltip } from "./Tooltip";

type SettingsPage =
  | "general"
  | "shortcuts"
  | "subtitle"
  | "models"
  | "privacy"
  | "diagnostics"
  | "about";

interface SettingsAppProps {
  initialPage?: SettingsPage;
}

const PAGE_ITEMS: Array<{
  id: SettingsPage;
  label: string;
  icon: typeof Settings2;
}> = [
  { id: "general", label: "通用", icon: Settings2 },
  { id: "shortcuts", label: "快捷键", icon: Keyboard },
  { id: "subtitle", label: "默认字幕样式", icon: SlidersHorizontal },
  { id: "models", label: "本地模型", icon: AudioLines },
  { id: "privacy", label: "隐私", icon: LockKeyhole },
  { id: "diagnostics", label: "诊断", icon: ClipboardCheck },
  { id: "about", label: "关于", icon: Info },
];

const DEFAULT_PREFERENCES: api.AppPreferencesV1 = {
  schema_version: 1,
  theme: "light",
  restore_last_project: true,
  timecode_precision: "frame",
  project_library_view: "grid",
  history_limit: 200,
  transcript_section_tint: true,
  cjk_spacing: true,
  default_subtitle_style: null,
  model_root: "~/Library/Application Support/Double Love/models",
  model_endpoint: "https://www.modelscope.cn",
  default_asr_model: "qwen3-asr-0.6b-4bit",
  onboarding_version: 1,
  onboarding_completed: false,
  recent_projects: [],
};

const PREVIEW_MODELS: api.ModelDescriptor[] = [
  {
    id: "qwen3-asr-0.6b-4bit",
    label: "Qwen3 ASR 0.6B · 4-bit",
    kind: "asr",
    ui_role: "primary",
    download_source: "modelscope",
    revision: "preview",
    size_bytes: 712_778_816,
    memory_bytes: 8_000_000_000,
    license: "Apache-2.0",
    description: "2 GB RAM • ~10x realtime • High accuracy",
    dependencies: [
      {
        model_id: "qwen3-forced-aligner-0.6b-8bit",
        required: true,
        reason: "逐词时间锚点",
      },
    ],
    replaces_model_id: "qwen3-asr-0.6b",
    state: "not_installed",
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
  {
    id: "qwen3-asr-1.7b-8bit",
    label: "Qwen3 ASR 1.7B · 8-bit",
    kind: "asr",
    ui_role: "primary",
    download_source: "modelscope",
    revision: "preview",
    size_bytes: 2_467_856_567,
    memory_bytes: 16_000_000_000,
    license: "Apache-2.0",
    description: "5 GB RAM • ~5x realtime • Highest accuracy",
    dependencies: [
      {
        model_id: "qwen3-forced-aligner-0.6b-8bit",
        required: true,
        reason: "逐词时间锚点",
      },
    ],
    replaces_model_id: "qwen3-asr-1.7b",
    state: "not_installed",
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
  {
    id: "wespeaker-voxceleb-resnet34-lm",
    label: "说话人识别 · MLX",
    kind: "speaker",
    ui_role: "primary",
    download_source: "modelscope",
    revision: "preview",
    size_bytes: 26_614_852,
    memory_bytes: 0,
    license: "MIT",
    description: "约 26 MB · 本地说话人区分 · MLX",
    dependencies: [
      {
        model_id: "silero-vad-v6",
        required: true,
        reason: "说话人区间需要 Silero VAD",
      },
    ],
    replaces_model_id: "wespeaker-zh",
    state: "not_installed",
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
];

function readableBytes(value: number | bigint): string {
  const bytes = num(value);
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let index = 0;
  let current = bytes;
  while (current >= 1024 && index < units.length - 1) {
    current /= 1024;
    index += 1;
  }
  return `${current >= 10 || index === 0 ? Math.round(current) : current.toFixed(1)} ${units[index]}`;
}

function stateLabel(state: api.ModelInstallState): string {
  return {
    not_installed: "未安装",
    queued: "排队中",
    downloading: "下载中",
    paused: "已暂停",
    verifying: "校验中",
    installed: "已安装",
    corrupt: "需要修复",
    failed: "下载失败",
  }[state];
}

function requiresNoncommercialConfirmation(model: api.ModelDescriptor): boolean {
  const license = model.license.toUpperCase();
  return license.includes("CC BY-NC-SA") || license.includes("RESEARCH-ONLY");
}

function wireInteger(value: number): bigint {
  // SAFETY: ts-rs declares JSON integer fields as bigint although the desktop wire format uses numbers.
  return value as unknown as bigint;
}

function defaultStyle(): SubtitleStyle {
  return {
    font_family: "PingFang SC",
    font_size: 46,
    font_weight: wireInteger(500),
    text_color: "#ffffff",
    outline_color: "#111318",
    outline_width: 3,
    shadow_color: "#00000080",
    shadow_offset_x: 0,
    shadow_offset_y: 2,
    shadow_blur: 4,
    background_color: "#11131800",
    background_radius: 8,
    background_padding_x: 10,
    background_padding_y: 6,
    position_x: 0.5,
    position_y: 0.84,
    max_width_ratio: 0.86,
    max_lines: wireInteger(2),
    target_characters_per_line: wireInteger(18),
    show_speaker: false,
    cjk_spacing: true,
  };
}

function usePreferences() {
  const [preferences, setPreferences] =
    useState<api.AppPreferencesV1>(DEFAULT_PREFERENCES);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!api.isDesktop) {
      setLoading(false);
      return;
    }
    try {
      const result = await api.preferencesGet();
      if (result.status === "success" && result.data)
        setPreferences(result.data);
      else setNotice(result.diagnostics[0]?.cause ?? "读取应用设置失败");
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "设置窗口暂时无法连接桌面服务",
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    if (!api.isDesktop) return;
    let disposed = false;
    let remove: (() => void) | undefined;
    void api
      .listen<{ changed_keys: string[] }>("dl://preferences-changed", () => {
        if (disposed) return;
        void api
          .preferencesGet()
          .then((result) => {
            if (!disposed && result.status === "success" && result.data)
              setPreferences(result.data);
          })
          .catch(() => undefined);
      })
      .then((unlisten) => {
        remove = unlisten;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      remove?.();
    };
  }, []);

  const update = useCallback(async (patch: api.PreferencesPatch) => {
    if (!api.isDesktop) {
      setPreferences((current) => ({ ...current, ...patch }));
      setNotice("浏览器预览：设置未写入桌面应用。");
      return;
    }
    try {
      const result = await api.preferencesUpdate(patch);
      if (result.status === "success" && result.data)
        setPreferences(result.data);
      else setNotice(result.diagnostics[0]?.cause ?? "设置没有保存");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "设置没有保存");
    }
  }, []);

  return { preferences, loading, notice, setNotice, update, reload };
}

export function SettingsApp({ initialPage = "general" }: SettingsAppProps) {
  const [page, setPage] = useState<SettingsPage>(initialPage);
  const { preferences, loading, notice, setNotice, update } = usePreferences();
  const [models, setModels] = useState<api.ModelDescriptor[]>([]);
  const [modelQueue, setModelQueue] = useState<api.ModelQueueSnapshot>({
    active_model_id: null,
    entries: [],
  });
  const [modelsLoading, setModelsLoading] = useState(true);
  const [modelBusy, setModelBusy] = useState<string | null>(null);
  const [noncommercialConfirmation, setNoncommercialConfirmation] = useState<{
    model: api.ModelDescriptor;
    action: "install" | "import";
  } | null>(null);
  const [legacyCleanup, setLegacyCleanup] = useState<{
    model: api.ModelDescriptor;
    preview: api.LegacyModelCleanupPreview;
  } | null>(null);
  const dismissedCleanupPrompts = useRef(new Set<string>());
  const [systemProfile, setSystemProfile] = useState<api.SystemProfile | null>(
    null,
  );
  const [doctor, setDoctor] = useState<api.DoctorReport | null>(null);
  const [doctorLoading, setDoctorLoading] = useState(false);
  const preview = !api.isDesktop;

  const loadModels = useCallback(async () => {
    if (!api.isDesktop) {
      setModels(PREVIEW_MODELS);
      setModelsLoading(false);
      return;
    }
    try {
      const result = await api.modelCatalog();
      if (result.status === "success") setModels(result.data ?? []);
      else setNotice(result.diagnostics[0]?.cause ?? "读取模型清单失败");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "模型服务暂时不可用");
    } finally {
      setModelsLoading(false);
    }
  }, [setNotice]);

  const openLegacyCleanup = useCallback(
    async (model: api.ModelDescriptor, explicit = false) => {
      if (!api.isDesktop || !model.replaces_model_id) return;
      if (!explicit && dismissedCleanupPrompts.current.has(model.id)) return;
      try {
        const result = await api.modelLegacyCleanupPreview(model.id);
        if (result.status !== "success" || !result.data) {
          if (explicit)
            setNotice(result.diagnostics[0]?.cause ?? "无法读取旧模型版本");
          return;
        }
        if (result.data.removable.length === 0) {
          if (explicit) setNotice("没有可清理的旧模型版本。");
          return;
        }
        setLegacyCleanup({ model, preview: result.data });
      } catch (error) {
        if (explicit)
          setNotice(error instanceof Error ? error.message : "无法读取旧模型版本");
      }
    },
    [setNotice],
  );

  useEffect(() => {
    void loadModels();
    if (api.isDesktop) {
      void api.modelQueueGet().then((result) => {
        if (result.status === "success" && result.data) setModelQueue(result.data);
      });
    }
  }, [loadModels]);

  useEffect(() => {
    if (!api.isDesktop) return;
    let disposed = false;
    const removers: Array<() => void> = [];
    void Promise.all([
      api.listen<
        Partial<api.ModelDownloadProgress> & {
          bytes_downloaded?: number | bigint;
          bytes_total?: number | bigint;
        }
      >("dl://model-progress", (event) => {
        if (disposed) return;
        const progress = api.normalizeModelProgress(event.payload);
        setModels((current) =>
          current.map((model) =>
            model.id === progress.model_id
              ? {
                  ...model,
                  state: progress.state,
                  downloaded_bytes: progress.completed_bytes,
                }
              : model,
          ),
        );
      }),
      api.listen<
        Partial<api.ModelInstallation> & {
          bytes_downloaded?: number | bigint;
          bytes_total?: number | bigint;
        }
      >("dl://model-state", (event) => {
        if (disposed) return;
        const installation = api.normalizeModelInstallation(event.payload);
        void loadModels();
        if (installation.state !== "installed") return;
        void api.modelCatalog().then((result) => {
          if (disposed || result.status !== "success") return;
          const model = result.data?.find(
            (item) => item.id === installation.model_id,
          );
          if (model) void openLegacyCleanup(model);
        });
      }),
      api.listen<api.ModelQueueSnapshot>("dl://model-queue", (event) => {
        if (!disposed) setModelQueue(event.payload);
      }),
    ])
      .then((unlisten) => removers.push(...unlisten))
      .catch(() => undefined);
    return () => {
      disposed = true;
      removers.forEach((remove) => remove());
    };
  }, [loadModels, openLegacyCleanup]);

  useEffect(() => {
    if (!api.isDesktop) return;
    void api
      .systemProfile()
      .then((result) => {
        if (result.status === "success") setSystemProfile(result.data ?? null);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.classList.toggle(
        "dark",
        preferences.theme === "dark" ||
          (preferences.theme === "system" && media.matches),
      );
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [preferences.theme]);

  const runDoctor = async (depth: "quick" | "deep" = "quick") => {
    if (!api.isDesktop) {
      setNotice("浏览器预览：诊断需要在桌面应用中运行。");
      return;
    }
    setDoctorLoading(true);
    try {
      const result = await api.doctorRun(depth);
      if (result.status === "success") setDoctor(result.data ?? null);
      else setNotice(result.diagnostics[0]?.cause ?? "诊断没有完成");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "诊断没有完成");
    } finally {
      setDoctorLoading(false);
    }
  };

  const performModelAction = async (
    model: api.ModelDescriptor,
    action: "install" | "pause" | "resume" | "cancel" | "verify" | "remove",
    acceptNoncommercialLicense = false,
  ) => {
    if (!api.isDesktop) {
      setNotice("浏览器预览：模型操作需要在桌面应用中执行。");
      return;
    }
    setModelBusy(model.id);
    try {
      const result =
        action === "install"
          ? await api.modelInstall(model.id, acceptNoncommercialLicense)
          : action === "pause"
            ? await api.modelPause(model.id)
            : action === "resume"
              ? await api.modelResume(model.id)
              : action === "cancel"
                ? await api.modelCancel(model.id)
                : action === "verify"
                  ? await api.modelVerify(model.id)
                  : await api.modelRemove(model.id);
      if (result.status === "failed")
        setNotice(result.diagnostics[0]?.cause ?? "模型操作没有完成");
      await loadModels();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "模型操作没有完成");
    } finally {
      setModelBusy(null);
    }
  };

  const requestModelAction = (
    model: api.ModelDescriptor,
    action: "install" | "pause" | "resume" | "cancel" | "verify" | "remove",
  ) => {
    if (action === "install" && requiresNoncommercialConfirmation(model)) {
      setNoncommercialConfirmation({ model, action: "install" });
      return;
    }
    void performModelAction(model, action);
  };

  const importModel = async (
    model: api.ModelDescriptor,
    acceptNoncommercialLicense = false,
  ) => {
    if (requiresNoncommercialConfirmation(model) && !acceptNoncommercialLicense) {
      setNoncommercialConfirmation({ model, action: "import" });
      return;
    }
    if (!api.isDesktop) {
      setNotice("浏览器预览：模型导入需要在桌面应用中执行。");
      return;
    }
    try {
      const grantToken = await api.pickDirectory(
        `选择“${model.label}”模型文件夹`,
        "model-import",
      );
      if (!grantToken) return;
      setModelBusy(model.id);
      const result = await api.modelImportFolder(
        model.id,
        grantToken,
        acceptNoncommercialLicense,
      );
      if (result.status === "failed")
        setNotice(result.diagnostics[0]?.cause ?? "模型文件夹导入失败");
      await loadModels();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "模型文件夹导入失败");
    } finally {
      setModelBusy(null);
    }
  };

  const revealModel = async (model: api.ModelDescriptor) => {
    if (!api.isDesktop) {
      setNotice("浏览器预览：需要在桌面应用中打开模型目录。");
      return;
    }
    try {
      const result = await api.modelReveal(model.id);
      if (result.status === "failed")
        setNotice(result.diagnostics[0]?.cause ?? "模型目录暂时无法打开");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "模型目录暂时无法打开");
    }
  };

  const applyLegacyCleanup = async () => {
    if (!legacyCleanup) return;
    const { model, preview } = legacyCleanup;
    if (!api.isDesktop) return;
    setModelBusy(model.id);
    try {
      const result = await api.modelLegacyCleanupApply(model.id);
      if (result.status === "success") {
        setLegacyCleanup(null);
        setNotice(
          preview.bytes_to_free > 0
            ? `已清理旧版本，释放 ${readableBytes(preview.bytes_to_free)}。`
            : "已清理旧版本。",
        );
      } else {
        setNotice(result.diagnostics[0]?.cause ?? "旧模型版本没有清理完成");
      }
      await loadModels();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "旧模型版本没有清理完成");
    } finally {
      setModelBusy(null);
    }
  };

  const resetOnboarding = async () => {
    if (!api.isDesktop) {
      window.dispatchEvent(new CustomEvent("dl://onboarding-reset"));
      setNotice("浏览器预览：下次打开桌面应用时会重新显示引导。");
      return;
    }
    try {
      const result = await api.onboardingReset();
      if (result.status === "success")
        setNotice("已重新打开新手引导；切回主窗口即可开始。");
      else setNotice(result.diagnostics[0]?.cause ?? "新手引导状态没有重置");
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "新手引导状态没有重置",
      );
    }
  };

  const currentStyle = preferences.default_subtitle_style ?? defaultStyle();
  const recommended =
    systemProfile?.recommended_asr_model ?? "qwen3-asr-0.6b-4bit";

  return (
    <div className="settings-window" aria-label="设置">
      <a className="studio-skip-link" href="#settings-main">
        跳到主要内容
      </a>
      <div className="settings-layout">
        <aside className="settings-sidebar" aria-label="设置分类">
          <nav>
            {PAGE_ITEMS.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                type="button"
                className={page === id ? "is-active" : ""}
                aria-current={page === id ? "page" : undefined}
                onClick={() => setPage(id)}
              >
                <Icon size={14} strokeWidth={1.8} />
                <span>{label}</span>
              </button>
            ))}
          </nav>
        </aside>
        <main className="settings-content" id="settings-main" tabIndex={-1}>
          {preview && (
            <div className="settings-preview-banner" role="status">
              浏览器预览：读取和操作模型、偏好与诊断需要桌面应用。
            </div>
          )}
          {notice && (
            <div
              className="settings-inline-notice"
              role="status"
              aria-live="polite"
            >
              <span>{notice}</span>
              <button
                type="button"
                aria-label="关闭提示"
                onClick={() => setNotice(null)}
              >
                ×
              </button>
            </div>
          )}
          {loading ? (
            <div className="settings-loading" role="status">
              正在读取设置…
            </div>
          ) : (
            <>
              {page === "general" && (
                <GeneralPage
                  preferences={preferences}
                  onUpdate={update}
                  onResetOnboarding={() => void resetOnboarding()}
                />
              )}
              {page === "shortcuts" && <ShortcutsPage />}
              {page === "subtitle" && (
                <SubtitlePage
                  style={currentStyle}
                  onUpdate={(style) =>
                    update({ default_subtitle_style: style })
                  }
                  onApply={async () => {
                    if (!api.isDesktop) {
                      setNotice("浏览器预览：需要在桌面应用中应用到当前项目。");
                      return;
                    }
                    try {
                      const result = await api.applyDefaultSubtitleStyle();
                      setNotice(
                        result.status === "success"
                          ? "已应用到当前项目。"
                          : (result.diagnostics[0]?.cause ??
                              "没有可应用的当前项目"),
                      );
                    } catch (error) {
                      setNotice(
                        error instanceof Error
                          ? error.message
                          : "没有可应用的当前项目",
                      );
                    }
                  }}
                />
              )}
              {page === "models" && (
                <ModelsPage
                  models={models}
                  modelsLoading={modelsLoading}
                  modelBusy={modelBusy}
                  queue={modelQueue}
                  recommended={recommended}
                  onAction={requestModelAction}
                  onImport={(model) => void importModel(model)}
                  onReveal={(model) => void revealModel(model)}
                  onCleanup={(model) => void openLegacyCleanup(model, true)}
                />
              )}
              {page === "privacy" && <PrivacyPage />}
              {page === "diagnostics" && (
                <DiagnosticsPage
                  doctor={doctor}
                  loading={doctorLoading}
                  onRun={() => void runDoctor("quick")}
                  onDeepRun={() => void runDoctor("deep")}
                  onReveal={() => {
                    if (api.isDesktop)
                      void api
                        .diagnosticsRevealLogs()
                        .catch(() => setNotice("日志目录暂时无法打开"));
                    else
                      setNotice("浏览器预览：日志目录需要在桌面应用中打开。");
                  }}
                />
              )}
              {page === "about" && <AboutPage />}
            </>
          )}
          {noncommercialConfirmation && (
            <NoncommercialLicenseDialog
              model={noncommercialConfirmation.model}
              busy={modelBusy === noncommercialConfirmation.model.id}
              onCancel={() => setNoncommercialConfirmation(null)}
              onConfirm={() => {
                const { model, action } = noncommercialConfirmation;
                setNoncommercialConfirmation(null);
                if (action === "install")
                  void performModelAction(model, "install", true);
                else void importModel(model, true);
              }}
            />
          )}
          {legacyCleanup && (
            <LegacyCleanupDialog
              model={legacyCleanup.model}
              preview={legacyCleanup.preview}
              busy={modelBusy === legacyCleanup.model.id}
              onCancel={() => {
                dismissedCleanupPrompts.current.add(legacyCleanup.model.id);
                setLegacyCleanup(null);
              }}
              onConfirm={() => void applyLegacyCleanup()}
            />
          )}
        </main>
      </div>
    </div>
  );
}

function PageHeader({
  title,
}: {
  title: string;
  description: string;
}) {
  return (
    <header className="settings-page-header">
      <h1>{title}</h1>
    </header>
  );
}

function SettingRow({
  title,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="settings-row">
      <div>
        <strong>{title}</strong>
      </div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
}) {
  return (
    <label className="settings-toggle">
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span aria-hidden="true" />
    </label>
  );
}

function GeneralPage({
  preferences,
  onUpdate,
  onResetOnboarding,
}: {
  preferences: api.AppPreferencesV1;
  onUpdate: (patch: api.PreferencesPatch) => void;
  onResetOnboarding: () => void;
}) {
  const [historyConfirmation, setHistoryConfirmation] = useState<{
    limit: number | null;
    removed: number;
  } | null>(null);
  const chooseHistoryLimit = async (value: string) => {
    const limit = value === "unlimited" ? null : Number(value);
    if (!api.isDesktop) {
      onUpdate({ history_limit: limit });
      return;
    }
    const preview = await api.historyLimitPreview(limit);
    const removed = preview.status === "success" ? Number(preview.data ?? 0) : 0;
    if (removed > 0) setHistoryConfirmation({ limit, removed });
    else onUpdate({ history_limit: limit });
  };
  return (
    <section className="settings-page" aria-labelledby="general-title">
      <PageHeader
        title="通用"
        description="调整应用在这台 Mac 上的工作方式。"
      />
      <div className="settings-group-title">启动</div>
      <SettingRow
        title="打开应用时恢复上次项目"
        description="从最近一次编辑的位置继续。"
      >
        <Toggle
          checked={preferences.restore_last_project}
          label="打开应用时恢复上次项目"
          onChange={(value) => onUpdate({ restore_last_project: value })}
        />
      </SettingRow>
      <SettingRow title="显示新手引导" description="可随时重新查看三步介绍。">
        <button
          type="button"
          className="settings-text-button"
          onClick={onResetOnboarding}
        >
          重新打开
        </button>
      </SettingRow>
      <div className="settings-group-title">编辑</div>
      <SettingRow title="回滚上限">
        <select
          aria-label="回滚上限"
          value={preferences.history_limit ?? "unlimited"}
          onChange={(event) => void chooseHistoryLimit(event.target.value)}
        >
          {[50, 100, 200, 500, 1000].map((limit) => (
            <option key={limit} value={limit}>{limit} 个版本</option>
          ))}
          <option value="unlimited">不设上限</option>
        </select>
      </SettingRow>
      {historyConfirmation && (
        <div className="settings-history-confirmation" role="alertdialog" aria-label="确认回滚上限">
          <span>将移除 {historyConfirmation.removed} 个旧恢复快照，审计记录仍会保留。</span>
          <div>
            <button type="button" onClick={() => setHistoryConfirmation(null)}>取消</button>
            <button
              type="button"
              className="is-danger"
              onClick={() => {
                onUpdate({ history_limit: historyConfirmation.limit });
                setHistoryConfirmation(null);
              }}
            >确认清理</button>
          </div>
        </div>
      )}
      <SettingRow
        title="转录分区底色"
        description="帮助你在文本中定位当前片段。"
      >
        <Toggle
          checked={preferences.transcript_section_tint}
          label="转录分区底色"
          onChange={(value) => onUpdate({ transcript_section_tint: value })}
        />
      </SettingRow>
      <SettingRow
        title="中日韩文字间距"
        description="导出字幕时在中日韩字符之间加入可读间距。"
      >
        <Toggle
          checked={preferences.cjk_spacing}
          label="中日韩文字间距"
          onChange={(value) => onUpdate({ cjk_spacing: value })}
        />
      </SettingRow>
      <div className="settings-group-title">界面</div>
      <SettingRow title="外观" description="亮色界面适合长时间剪辑。">
        <select
          aria-label="应用主题"
          value={preferences.theme}
          onChange={(event) =>
            onUpdate({ theme: event.target.value as api.ThemeMode })
          }
        >
          <option value="light">亮色</option>
          <option value="dark">深色</option>
          <option value="system">跟随系统</option>
        </select>
      </SettingRow>
    </section>
  );
}

const SHORTCUTS = [
  ["新建项目", "⌘ N"],
  ["打开项目", "⌘ O"],
  ["设置", "⌘ ,"],
  ["播放 / 暂停", "Space"],
  ["前后跳转", "← / →"],
  ["拆分", "S"],
  ["撤销", "⌘ Z"],
  ["重做", "⇧ ⌘ Z"],
  ["导出", "⌘ E"],
];

function ShortcutsPage() {
  return (
    <section className="settings-page" aria-labelledby="shortcuts-title">
      <PageHeader
        title="快捷键"
        description="固定快捷键已经接入编辑器；首版不提供改键入口。"
      />
      <div className="settings-shortcut-list">
        {SHORTCUTS.map(([label, key]) => (
          <div key={label} className="settings-shortcut-row">
            <span>{label}</span>
            <kbd>{key}</kbd>
          </div>
        ))}
      </div>
      <p className="settings-footnote">
        快捷键遵循 macOS
        习惯。文本输入框获得焦点时，播放和编辑动作不会抢走按键。
      </p>
    </section>
  );
}

function SubtitlePage({
  style,
  onUpdate,
  onApply,
}: {
  style: SubtitleStyle;
  onUpdate: (style: SubtitleStyle) => void;
  onApply: () => void;
}) {
  const update = (patch: Partial<SubtitleStyle>) =>
    onUpdate({ ...style, ...patch });
  return (
    <section className="settings-page" aria-labelledby="subtitle-title">
      <PageHeader
        title="默认字幕样式"
        description="只影响新建项目；已有项目不会被自动改写。"
      />
      <div className="settings-callout">
        <Wand2 size={15} />
        <span>当前项目的字幕样式仍在编辑器右侧单独调整。</span>
      </div>
      <div className="settings-group-title">文字</div>
      <SettingRow
        title="字体"
        description="使用系统字体，确保中文输入法和导出一致。"
      >
        <select
          aria-label="默认字幕字体"
          value={style.font_family}
          onChange={(event) => update({ font_family: event.target.value })}
        >
          <option>PingFang SC</option>
          <option>Hiragino Sans GB</option>
          <option>Helvetica Neue</option>
        </select>
      </SettingRow>
      <SettingRow title="字号" description="以像素为单位。">
        <input
          aria-label="默认字幕字号"
          type="number"
          min="12"
          max="160"
          value={style.font_size}
          onChange={(event) =>
            update({
              font_size: Math.max(
                12,
                Number(event.target.value) || style.font_size,
              ),
            })
          }
        />
      </SettingRow>
      <SettingRow title="每行目标字数" description="用于生成新项目的默认换行。">
        <input
          aria-label="默认字幕每行目标字数"
          type="number"
          min="4"
          max="80"
          value={num(style.target_characters_per_line)}
          onChange={(event) =>
            update({
              target_characters_per_line: wireInteger(
                Math.max(
                  4,
                  Math.round(
                    Number(event.target.value) ||
                      num(style.target_characters_per_line),
                  ),
                ),
              ),
            })
          }
        />
      </SettingRow>
      <SettingRow title="显示说话人名称">
        <Toggle
          checked={style.show_speaker}
          label="默认显示说话人名称"
          onChange={(value) => update({ show_speaker: value })}
        />
      </SettingRow>
      <div className="settings-group-title">预览与应用</div>
      <div
        className="settings-subtitle-preview"
        style={{
          color: style.text_color,
          fontSize: `${Math.min(32, Math.max(16, style.font_size / 2))}px`,
          textShadow: `0 1px ${style.outline_color}`,
        }}
      >
        这是新项目的字幕预览
      </div>
      <button
        type="button"
        className="settings-secondary-button"
        onClick={onApply}
      >
        在当前项目中应用
      </button>
    </section>
  );
}

interface ModelsPageProps {
  models: api.ModelDescriptor[];
  modelsLoading: boolean;
  modelBusy: string | null;
  queue?: api.ModelQueueSnapshot;
  recommended: string;
  onAction: (
    model: api.ModelDescriptor,
    action: "install" | "pause" | "resume" | "cancel" | "verify" | "remove",
  ) => void;
  onImport: (model: api.ModelDescriptor) => void;
  onReveal: (model: api.ModelDescriptor) => void;
  onCleanup: (model: api.ModelDescriptor) => void;
}

export function ModelsPage({
  models,
  modelsLoading,
  modelBusy,
  queue = { active_model_id: null, entries: [] },
  recommended,
  onAction,
  onImport,
  onReveal,
  onCleanup,
}: ModelsPageProps) {
  const [contextMenu, setContextMenu] = useState<{
    model: api.ModelDescriptor;
    left: number;
    top: number;
  } | null>(null);
  const primaryModels = models.filter((model) => model.ui_role === "primary");
  const legacyModels = models.filter(
    (model) => model.ui_role === "legacy" && model.state === "installed",
  );
  const byId = new Map(models.map((model) => [model.id, model]));
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setContextMenu(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
  const openContextMenu = (
    model: api.ModelDescriptor,
    left: number,
    top: number,
  ) => {
    setContextMenu({
      model,
      left: Math.max(8, Math.min(window.innerWidth - 210, left)),
      top: Math.max(8, Math.min(window.innerHeight - 70, top)),
    });
  };
  const importDisabled = contextMenu
    ? ["queued", "downloading", "paused", "verifying"].includes(
        contextMenu.model.state,
      )
    : false;
  const revealLabel = /Mac/i.test(navigator.platform)
    ? "在访达中显示"
    : "打开所在文件夹";
  return (
    <section className="settings-page" aria-labelledby="models-title">
      <PageHeader
        title="本地模型"
        description="模型权重保存在 Application Support，运行时保持离线。"
      />
      {modelsLoading ? (
        <div className="settings-loading-row">正在读取模型清单…</div>
      ) : (
        primaryModels.map((model) => (
          <ModelRow
            key={model.id}
            model={model}
            recommended={model.id === recommended}
            busy={modelBusy === model.id}
            dependencies={model.dependencies.flatMap((dependency) => {
              const resolved = byId.get(dependency.model_id);
              return resolved ? [{ ...dependency, model: resolved }] : [];
            })}
            downloadGroup={modelDownloadGroup(model, models)}
            downloadSize={modelDownloadSize(model, models)}
            queueEntry={queue.entries.find((entry) => entry.model_id === model.id)}
            onAction={onAction}
            contextModelId={contextMenu?.model.id ?? null}
            onOpenContextMenu={openContextMenu}
          />
        ))
      )}
      {!modelsLoading && primaryModels.length === 0 && (
        <p className="settings-footnote">
          当前没有可管理的模型。请运行诊断或检查本地运行时。
        </p>
      )}
      {legacyModels.length > 0 && (
        <section className="settings-legacy-models" aria-label="旧模型版本">
          <strong>旧模型需要升级</strong>
          <p>旧版本不会再用于安装或推理；安装对应的 MLX 模型后可在右键菜单中清理。</p>
          <ul>
            {legacyModels.map((model) => (
              <li key={model.id}>
                <span>{model.label}</span>
                <small>{readableBytes(model.size_bytes)} · 需要升级</small>
              </li>
            ))}
          </ul>
        </section>
      )}
      {contextMenu && (
        <div
          className="studio-context-menu-layer"
          role="presentation"
          onMouseDown={() => setContextMenu(null)}
          onContextMenu={(event) => event.preventDefault()}
        >
          <div
            className="studio-project-context-menu settings-model-context-menu"
            role="menu"
            aria-label={`${contextMenu.model.label} 模型操作`}
            style={{ left: contextMenu.left, top: contextMenu.top }}
            onMouseDown={(event) => event.stopPropagation()}
          >
            {contextMenu.model.state === "installed" ? (
              <>
                <button
                  type="button"
                  role="menuitem"
                  autoFocus
                  disabled={modelBusy === contextMenu.model.id}
                  onClick={() => {
                    const model = contextMenu.model;
                    setContextMenu(null);
                    onReveal(model);
                  }}
                >
                  {revealLabel}
                </button>
                {contextMenu.model.replaces_model_id && (
                  <button
                    type="button"
                    role="menuitem"
                    disabled={modelBusy === contextMenu.model.id}
                    onClick={() => {
                      const model = contextMenu.model;
                      setContextMenu(null);
                      onCleanup(model);
                    }}
                  >
                    清理旧版本…
                  </button>
                )}
              </>
            ) : (
              <>
                <button
                  type="button"
                  role="menuitem"
                  autoFocus
                  disabled={importDisabled || modelBusy === contextMenu.model.id}
                  onClick={() => {
                    const model = contextMenu.model;
                    setContextMenu(null);
                    onImport(model);
                  }}
                >
                  从文件夹导入…
                </button>
                {importDisabled && <small>请先结束当前下载或校验任务</small>}
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

function ModelRow({
  model,
  recommended,
  busy,
  dependencies,
  downloadGroup,
  downloadSize,
  queueEntry,
  onAction,
  contextModelId,
  onOpenContextMenu,
}: {
  model: api.ModelDescriptor;
  recommended: boolean;
  busy: boolean;
  dependencies: ResolvedModelDependency[];
  downloadGroup: ModelDownloadGroup | null;
  downloadSize: ModelDownloadSize;
  queueEntry?: api.ModelQueueEntry;
  onAction: ModelsPageProps["onAction"];
  contextModelId: string | null;
  onOpenContextMenu: (
    model: api.ModelDescriptor,
    left: number,
    top: number,
  ) => void;
}) {
  const primaryAction =
    model.state === "not_installed" ||
    model.state === "failed" ||
    model.state === "corrupt"
      ? "install"
      : model.state === "downloading" || model.state === "queued"
        ? "pause"
        : model.state === "paused"
          ? "resume"
          : null;
  const summary =
    model.id === "qwen3-asr-0.6b-4bit"
      ? "2 GB RAM • ~10x realtime • High accuracy"
      : model.id === "qwen3-asr-1.7b-8bit"
        ? "5 GB RAM • ~5x realtime • Highest accuracy"
        : model.id === "wespeaker-voxceleb-resnet34-lm"
          ? "约 26 MB · 本地说话人区分 · MLX"
        : (model.description ?? readableBytes(model.size_bytes));
  const visibleDependencies = visibleModelDependencies(model, dependencies);
  const groupDependencyIds = new Set(
    (downloadGroup?.members ?? [])
      .filter((member) => member.id !== model.id)
      .map((member) => member.id),
  );
  const activeGroupDependencyIds = new Set(
    (downloadGroup?.members ?? [])
      .filter(
        (member) =>
          member.id !== model.id &&
          ["queued", "downloading", "paused", "verifying"].includes(
            member.state,
          ),
      )
      .map((member) => member.id),
  );
  const phaseDescription = downloadGroup?.phase;
  const showGroupProgress = Boolean(
    downloadGroup &&
      (downloadGroup.active ||
        downloadGroup.members.some((member) => member.state === "paused")),
  );
  const displayState =
    queueEntry?.state === "queued"
      ? `队列第 ${queueEntry.position} 位`
      : downloadGroup?.active && model.state === "queued"
      ? "安装中"
      : stateLabel(model.state);
  const openMenuFromKeyboard = (
    event: ReactKeyboardEvent<HTMLElement>,
    target: api.ModelDescriptor,
  ) => {
    if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
      event.preventDefault();
      event.stopPropagation();
      const rect = event.currentTarget.getBoundingClientRect();
      onOpenContextMenu(target, rect.left + 24, rect.top + 30);
    }
  };
  return (
    <article
      className={`settings-model-row${contextModelId === model.id ? " is-context-selected" : ""}`}
      tabIndex={0}
      onContextMenu={(event) => {
        event.preventDefault();
        onOpenContextMenu(model, event.clientX, event.clientY);
      }}
      onKeyDown={(event) => openMenuFromKeyboard(event, model)}
    >
      <div className="settings-model-icon">
        <AudioLines size={15} />
      </div>
      <div className="settings-model-copy">
        <strong>
          {model.label}
          {recommended && <span className="settings-model-recommended">推荐</span>}
        </strong>
        <small>{summary}</small>
        {model.state === "not_installed" &&
          num(model.downloaded_bytes ?? 0) > 0 && (
            <span className="settings-model-resume-hint">
              已下载 {formatDownloadSizePair(downloadSize.completedBytes, downloadSize.totalBytes)}，可继续安装
            </span>
          )}
        {showGroupProgress && downloadGroup && (
          <div
            className="settings-model-progress"
            role="progressbar"
            aria-label={`${model.label}整体下载进度`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(downloadGroup.percent)}
            aria-valuetext={`${Math.round(downloadGroup.percent)}% · ${formatDownloadSizePair(downloadGroup.completedBytes, downloadGroup.totalBytes)} · ${phaseDescription ?? model.label}${downloadGroup.current && downloadGroup.current.id !== model.id ? ` · ${downloadGroup.current.label}` : ""}`}
          >
            <i style={{ width: `${downloadGroup.percent}%` }} />
          </div>
        )}
        {showGroupProgress && downloadGroup && (
          <span className="settings-model-download-size">
            {formatDownloadSizePair(downloadGroup.completedBytes, downloadGroup.totalBytes)}
          </span>
        )}
        {phaseDescription && downloadGroup?.current && (
          <span className="settings-model-download-phase">
            <strong>{phaseDescription}</strong>
            {downloadGroup.current.id !== model.id && (
              <small>{downloadGroup.current.label}</small>
            )}
          </span>
        )}
        {model.error && <em>{model.error}</em>}
        {visibleDependencies.map(({ model: dependency, reason, required }) => (
          activeGroupDependencyIds.has(dependency.id) ? null :
          <span
            className={`settings-model-dependency is-${dependency.state}${contextModelId === dependency.id ? " is-context-selected" : ""}`}
            key={dependency.id}
            tabIndex={0}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              onOpenContextMenu(dependency, event.clientX, event.clientY);
            }}
            onKeyDown={(event) => openMenuFromKeyboard(event, dependency)}
          >
            {dependency.label} · {stateLabel(dependency.state)}
            {required && dependency.state !== "installed" && reason &&
            (!groupDependencyIds.has(dependency.id) || dependency.state === "failed" || dependency.state === "corrupt")
              ? ` · ${reason}`
              : ""}
          </span>
        ))}
      </div>
      <span className={`settings-model-state is-${model.state}`}>
        {displayState}
      </span>
      <div className="settings-model-actions">
        {primaryAction && (
          <Tooltip label={primaryAction === "install" ? (model.state === "failed" || model.state === "corrupt" ? "重试安装" : "安装模型") : primaryAction === "resume" ? "继续下载" : "暂停下载"}><button
            type="button"
            className="settings-icon-action is-primary"
            aria-label={primaryAction === "install" ? (model.state === "failed" || model.state === "corrupt" ? `重试安装${model.label}` : `安装${model.label}`) : primaryAction === "resume" ? `继续下载${model.label}` : `暂停下载${model.label}`}
            disabled={busy}
            onClick={() => onAction(model, primaryAction)}
          >
            {primaryAction === "install" ? <Download size={14} /> : primaryAction === "resume" ? <Play size={14} /> : <Pause size={14} />}
          </button></Tooltip>
        )}
        {model.state === "installed" && (
          <Tooltip label="校验模型"><button
            type="button"
            className="settings-icon-action"
            aria-label={`校验${model.label}`}
            disabled={busy}
            onClick={() => onAction(model, "verify")}
          >
            <RefreshCw size={14} />
          </button></Tooltip>
        )}
        {(model.state === "downloading" ||
          model.state === "paused" ||
          model.state === "queued") && (
          <Tooltip label="取消下载"><button
            type="button"
            className="settings-icon-action"
            aria-label={`取消下载${model.label}`}
            disabled={busy}
            onClick={() => onAction(model, "cancel")}
          >
            <X size={14} />
          </button></Tooltip>
        )}
        {model.state === "installed" && model.can_remove !== false && (
          <Tooltip label="删除模型"><button
            type="button"
            className="settings-icon-action"
            aria-label={`删除${model.label}`}
            disabled={busy}
            onClick={() => onAction(model, "remove")}
          >
            <Trash2 size={14} />
          </button></Tooltip>
        )}
        {model.state === "installed" && model.can_remove === false && (
          <span className="settings-cannot-remove">不可移除</span>
        )}
      </div>
    </article>
  );
}

function NoncommercialLicenseDialog({
  model,
  busy,
  onCancel,
  onConfirm,
}: {
  model: api.ModelDescriptor;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="studio-popover-backdrop" role="presentation" onMouseDown={onCancel}>
      <section
        className="settings-license-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="noncommercial-license-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="noncommercial-license-title">仅限非商业使用</h2>
        <p>
          {model.license.toUpperCase().includes("CC BY-NC-SA")
            ? `${model.label} 使用 CC BY-NC-SA 4.0 权重。仅在非商业项目中继续，且需保留署名并遵守相同方式共享要求。`
            : `${model.label} 使用仅限研究、禁止商业用途的权重。请确认本项目仅用于非商业场景。`}
        </p>
        <div>
          <button type="button" className="settings-secondary-button" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="settings-primary-button"
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? "准备中…" : "确认仅用于非商业项目"}
          </button>
        </div>
      </section>
    </div>
  );
}

export function LegacyCleanupDialog({
  model,
  preview,
  busy,
  onCancel,
  onConfirm,
}: {
  model: api.ModelDescriptor;
  preview: api.LegacyModelCleanupPreview;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="studio-popover-backdrop" role="presentation" onMouseDown={onCancel}>
      <section
        className="settings-license-dialog settings-legacy-cleanup-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="legacy-cleanup-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="legacy-cleanup-title">清理旧版本？</h2>
        <p>
          {model.label} 已安装。将移除以下旧模型文件，预计释放
          {` ${readableBytes(preview.bytes_to_free)} `}；不会删除原始媒体或导入源文件夹。
        </p>
        <ul className="settings-legacy-cleanup-list">
          {preview.removable.map((item) => (
            <li key={item.model_id}>
              <span>{item.display_name}</span>
              <small>{readableBytes(item.bytes_to_free)}</small>
            </li>
          ))}
        </ul>
        {preview.retained.length > 0 && (
          <p className="settings-footnote">
            {preview.retained.map((item) => `${item.display_name}：${item.reason}`).join("；")}
          </p>
        )}
        <div>
          <button type="button" className="settings-secondary-button" onClick={onCancel}>
            保留旧版本
          </button>
          <button
            type="button"
            className="settings-primary-button"
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? "正在清理…" : "清理旧版本"}
          </button>
        </div>
      </section>
    </div>
  );
}

function PrivacyPage() {
  return (
    <section className="settings-page" aria-labelledby="privacy-title">
      <PageHeader
        title="隐私"
        description="Double Love 的默认工作方式是本地优先。"
      />
      <div className="settings-privacy-list">
        <div>
          <LockKeyhole size={16} />
          <div>
            <strong>没有默认遥测</strong>
            <p>应用不会自动发送使用数据、崩溃报告或项目路径。</p>
          </div>
        </div>
        <div>
          <AudioLines size={16} />
          <div>
            <strong>音频和声纹不上传</strong>
            <p>转录、说话人识别和声纹只在本机运行并保存在项目中。</p>
          </div>
        </div>
        <div>
          <Wand2 size={16} />
          <div>
            <strong>Agent 数据包需要预览确认</strong>
            <p>外部名称建议只允许你查看并确认最小匿名文本。</p>
          </div>
        </div>
        <div>
          <FolderOpen size={16} />
          <div>
            <strong>本地数据</strong>
            <p>偏好、模型和日志都位于你的 Mac。打开目录前会由系统确认。</p>
          </div>
        </div>
      </div>
    </section>
  );
}

const CAPABILITY_LABELS: Record<string, string> = {
  "media.ffmpeg_runtime": "App 内置 ffmpeg",
  "media.ffprobe_runtime": "App 内置 ffprobe",
  "media.ass_filter": "字幕烧录能力",
  "media.h264_encoder": "H.264 导出能力",
  "media.aac_encoder": "AAC 导出能力",
  "runtime.asr": "App 内置 ASR 运行时",
  "runtime.speaker": "App 内置 Speaker 运行时",
  "system.mlx_platform": "Apple Silicon / macOS",
  "storage.model_root": "模型目录",
  "models.asr_chain": "转录模型依赖链",
  "models.speaker_chain": "说话人模型依赖链",
  "deep.media_render": "深度媒体编码试跑",
  "deep.asr": "深度 ASR 试跑",
  "deep.speaker": "深度 Speaker 试跑",
};

function capabilityLabel(id: string): string {
  return CAPABILITY_LABELS[id] ?? id;
}

function capabilityStatusLabel(status: api.DoctorCapabilityStatus): string {
  return {
    ready: "可用",
    warning: "未就绪",
    blocked: "阻断",
    not_run: "未执行",
  }[status];
}

function DiagnosticsPage({
  doctor,
  loading,
  onRun,
  onDeepRun,
  onReveal,
}: {
  doctor: api.DoctorReport | null;
  loading: boolean;
  onRun: () => void;
  onDeepRun: () => void;
  onReveal: () => void;
}) {
  const mediaReady = doctor?.ffmpeg === "可用" && doctor.libass === "可用";
  const runtimeReady = doctor
    ? !doctor.offline_runtime.includes("不可用")
    : false;
  return (
    <section className="settings-page" aria-labelledby="diagnostics-title">
      <PageHeader
        title="诊断"
        description="检查离线运行环境、模型完整性和可用空间。"
      />
      <div className="settings-diagnostics-actions">
        <button
          type="button"
          className="settings-primary-button"
          disabled={loading}
          onClick={onRun}
        >
          <RefreshCw size={14} />
          {loading ? "检查中…" : "运行诊断"}
        </button>
        <button
          type="button"
          className="settings-secondary-button"
          disabled={loading}
          onClick={onDeepRun}
        >
          <RefreshCw size={14} />
          {loading ? "检查中…" : "深度诊断"}
        </button>
        <button
          type="button"
          className="settings-secondary-button"
          onClick={onReveal}
        >
          <FolderOpen size={14} />
          打开日志目录
        </button>
      </div>
      {doctor ? (
        <div className="settings-doctor-report">
          <SettingRow
            title="应用与系统"
            description={`${doctor.app_version} · ${doctor.architecture} · ${doctor.os_version}`}
          >
            <Check size={15} className="settings-ok" />
          </SettingRow>
          <SettingRow
            title="ffmpeg / libass"
            description={`${doctor.ffmpeg} / ${doctor.libass}`}
          >
            {mediaReady ? (
              <Check size={15} className="settings-ok" />
            ) : (
              <AlertTriangle size={15} className="settings-warning" />
            )}
          </SettingRow>
          <SettingRow title="离线运行时" description={doctor.offline_runtime}>
            {runtimeReady ? (
              <Check size={15} className="settings-ok" />
            ) : (
              <AlertTriangle size={15} className="settings-warning" />
            )}
          </SettingRow>
          <SettingRow
            title="可用磁盘"
            description={readableBytes(doctor.free_disk_bytes)}
          >
            <span className="settings-mono-value">
              {readableBytes(doctor.free_disk_bytes)}
            </span>
          </SettingRow>
          {doctor.capability_checks.length > 0 && (
            <div className="settings-doctor-capabilities">
              {doctor.capability_checks.map((check) => (
                <div key={check.id} className="settings-doctor-capability">
                  <div>
                    <strong>{capabilityLabel(check.id)}</strong>
                    <small>{check.detail}</small>
                    {check.suggested_action && (
                      <em>{check.suggested_action}</em>
                    )}
                  </div>
                  {check.status === "ready" ? (
                    <Check size={15} className="settings-ok" />
                  ) : check.status === "not_run" ? (
                    <Info size={15} />
                  ) : (
                    <AlertTriangle size={15} className="settings-warning" />
                  )}
                  <span>{capabilityStatusLabel(check.status)}</span>
                </div>
              ))}
            </div>
          )}
          <div className="settings-model-integrity">
            {doctor.model_integrity.map((model) => (
              <div key={model.model_id}>
                <span>{model.model_id}</span>
                <strong>
                  {stateLabel(model.state as api.ModelInstallState) ??
                    model.state}
                </strong>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div className="settings-empty-state">
          <ClipboardCheck size={18} />
          <strong>还没有诊断报告</strong>
          <p>运行一次检查，结果只保存在本地。</p>
        </div>
      )}
    </section>
  );
}

function updateStatusText(status: api.UpdateStatus): string {
  if (status.stage === "checking-update") return "正在检查更新…";
  if (status.stage === "update-available")
    return status.version ? `发现新版本 ${status.version}` : "发现新版本";
  if (status.stage === "update-not-available") return "当前已是最新版本。";
  if (status.stage === "download-progress")
    return `正在下载更新${status.percent === undefined ? "…" : ` ${status.percent.toFixed(1)}%`}`;
  if (status.stage === "update-downloaded") return "更新已下载，可以重启安装。";
  if (status.stage === "error")
    return status.error ?? "暂时无法完成更新操作，请稍后重试。";
  return "可手动检查新版本。";
}

function AboutPage() {
  const [appInfo, setAppInfo] = useState<api.AppInfo>({
    name: "Double Love Studio",
    version: "0.2.0",
  });
  const [updateStatus, setUpdateStatus] = useState<api.UpdateStatus>({
    stage: "idle",
  });

  useEffect(() => {
    let disposed = false;
    void api
      .getAppInfo()
      .then((info) => {
        if (!disposed) setAppInfo(info);
      })
      .catch(() => undefined);
    if (!api.isDesktop)
      return () => {
        disposed = true;
      };

    let remove: (() => void) | undefined;
    void api
      .listen<api.UpdateStatus>("dl://update-status", (event) => {
        if (!disposed) setUpdateStatus(event.payload);
      })
      .then((unlisten) => {
        remove = unlisten;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      remove?.();
    };
  }, []);

  const checkForUpdates = async () => {
    setUpdateStatus({ stage: "checking-update" });
    try {
      setUpdateStatus(await api.updateCheck());
    } catch {
      setUpdateStatus({
        stage: "error",
        error: "暂时无法检查更新，请稍后重试。",
      });
    }
  };

  const downloadUpdate = async () => {
    if (
      !window.confirm(
        "确认下载这个更新吗？下载完成后仍需再次确认才会重启安装。",
      )
    )
      return;
    try {
      setUpdateStatus(await api.updateDownload());
    } catch {
      setUpdateStatus({
        stage: "error",
        error: "暂时无法下载更新，请稍后重试。",
      });
    }
  };

  const installUpdate = async () => {
    if (!window.confirm("确认退出 Double Love Studio 并安装更新吗？")) return;
    try {
      setUpdateStatus(await api.updateInstall());
    } catch {
      setUpdateStatus({
        stage: "error",
        error: "暂时无法安装更新，请稍后重试。",
      });
    }
  };

  const busy =
    updateStatus.stage === "checking-update" ||
    updateStatus.stage === "download-progress";
  return (
    <section className="settings-page" aria-labelledby="about-title">
      <PageHeader
        title="关于"
        description="Double Love Studio · 本地粗剪工作台。"
      />
      <div className="settings-about-mark">
        <span>⌃</span>
        <div>
          <strong>{appInfo.name}</strong>
          <small>版本 {appInfo.version}</small>
        </div>
      </div>
      <div className="settings-group-title">应用更新</div>
      <div className="settings-diagnostics-actions">
        <button
          type="button"
          className="settings-secondary-button"
          disabled={busy}
          onClick={() => void checkForUpdates()}
        >
          <RefreshCw size={14} />
          检查更新
        </button>
        <button
          type="button"
          className="settings-primary-button"
          disabled={updateStatus.stage !== "update-available"}
          onClick={() => void downloadUpdate()}
        >
          下载更新
        </button>
        <button
          type="button"
          className="settings-primary-button"
          disabled={updateStatus.stage !== "update-downloaded"}
          onClick={() => void installUpdate()}
        >
          重启安装
        </button>
      </div>
      <p className="settings-footnote" role="status" aria-live="polite">
        {updateStatusText(updateStatus)}
      </p>
      <div className="settings-about-list">
        <SettingRow
          title="本地处理"
          description="原始媒体不会被复制，模型运行时不主动联网。"
        >
          <Check className="settings-ok" size={15} />
        </SettingRow>
        <SettingRow
          title="模型许可"
          description="模型权重遵循各自数据集许可；诊断页保留完整组件与版本信息。"
        >
          <ChevronRight size={15} />
        </SettingRow>
        <SettingRow
          title="第三方许可"
          description="查看构建中使用的开源组件和版本。"
        >
          <ChevronRight size={15} />
        </SettingRow>
      </div>
      <p className="settings-footnote">
        感谢你用本地工具整理声音和故事。问题反馈请附上诊断页中经过脱敏的报告。
      </p>
    </section>
  );
}

export { PAGE_ITEMS };
