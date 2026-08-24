import { randomUUID } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { once } from "node:events";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import Ajv2020, { type ValidateFunction } from "ajv/dist/2020.js";
import electronUpdater from "electron-updater";
import {
  app,
  BrowserWindow,
  dialog,
  Menu,
  net,
  protocol,
  session,
  shell,
  type IpcMainInvokeEvent,
  type MenuItemConstructorOptions,
} from "electron";
import type { HostRequest } from "../../../bindings/host-protocol/HostRequest";
import type { HostResponse } from "../../../bindings/host-protocol/HostResponse";
import { applyGrantPolicy } from "./grant-policy";
import { assertCompatibleHostHello } from "./host-handshake";
import {
  registerGuardedHandler,
  type GuardedIpcOptions,
  type WindowRole,
} from "./ipc-guard";
import { LocalLog } from "./local-log";
import { mediaNotFound, mediaResponse } from "./media-response";
import { isInvalidGrant, PathGrants, type GrantKind } from "./path-grants";
import { resolveProjectTarget } from "./project-creation";
import { projectIdFromThumbnailUrl } from "./project-thumbnail";
import { createQuitFlowState, handleBeforeQuit } from "./quit-flow";
import { UpdaterController } from "./updater";

const { autoUpdater } = electronUpdater;
const nodeRequire = createRequire(import.meta.url);

interface NativePlayer {
  attach(handle: Buffer): void;
  setFrame(x: number, y: number, width: number, height: number): void;
  loadTimeline(clips: string, seconds: number): void;
  setSubtitle(config: string): void;
  setPresentation(config: string): void;
  isPlayable(path: string): boolean;
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  state(): { state: string; seconds: number; duration: number; rate: number; ready_for_display: boolean; error: string };
  dispose(): void;
}

let nativePlayer: NativePlayer | null = null;
let playerPoll: NodeJS.Timeout | null = null;

function loadNativePlayer(): NativePlayer {
  if (nativePlayer) return nativePlayer;
  const path = app.isPackaged
    ? join(process.resourcesPath, "native/avfoundation_player.node")
    : join(repositoryRoot, "studio/build/native/avfoundation_player.node");
  nativePlayer = nodeRequire(path) as NativePlayer;
  return nativePlayer;
}

protocol.registerSchemesAsPrivileged([
  {
    scheme: "dl-app",
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      bypassCSP: false,
    },
  },
  {
    scheme: "dl-media",
    privileges: {
      standard: false,
      secure: true,
      supportFetchAPI: true,
      stream: true,
      bypassCSP: false,
    },
  },
  {
    scheme: "dl-thumbnail",
    privileges: {
      standard: false,
      secure: true,
      supportFetchAPI: true,
      stream: true,
      bypassCSP: false,
    },
  },
]);

const PROTOCOL_VERSION = 1;
const MAX_FRAME_BYTES = 64 * 1024 * 1024;
const REQUEST_TIMEOUT_MS = 5_000;
const SHUTDOWN_TIMEOUT_MS = 1_500;
const SETTINGS_QUERY = "window=settings";
const RENDERER_ORIGIN = "dl-app://app";
const E2E_SWITCH = "double-love-e2e";
const E2E_USER_DATA_SWITCH = "double-love-e2e-user-data";
const E2E_TRANSCRIBE_MOCK_SWITCH = "double-love-e2e-transcribe-mock";
const E2E_SPEAKER_MOCK_SWITCH = "double-love-e2e-speaker-mock";
const BOTH_WINDOWS: readonly WindowRole[] = ["main", "settings"];

// Renderer-reachable host commands. Excludes main-only resolution helpers and
// retired internal commands that never entered Electron (capability-matrix §1).
const RENDERER_COMMANDS: ReadonlySet<string> = new Set([
  "preferences_get",
  "preferences_update",
  "recent_projects_list",
  "recent_project_open",
  "project_checkpoint",
  "project_close",
  "recent_project_forget",
  "system_profile",
  "onboarding_get",
  "onboarding_complete",
  "onboarding_reset",
  "model_catalog",
  "model_queue_get",
  "model_install",
  "model_pause",
  "model_resume",
  "model_cancel",
  "model_verify",
  "model_remove",
  "model_legacy_cleanup_preview",
  "model_legacy_cleanup_apply",
  "model_import_folder",
  "model_reveal",
  "doctor_run",
  "diagnostics_reveal_logs",
  "project_open",
  "import_media",
  "assets_list",
  "media_asset_remove",
  "transcribe_start",
  "task_cancel",
  "project_revision",
  "project_history",
  "history_limit_preview",
  "project_restore_revision",
  "edit_undo",
  "edit_redo",
  "transcript_get",
  "edit_omit",
  "edit_restore",
  "roughcut_preview",
  "export_roughcut_apply",
  "project_export_preview",
  "project_export_xmeml_apply",
  "project_export_ass_apply",
  "project_render_mp4_apply",
  "timeline_get",
  "main_track_append",
  "main_track_append_full",
  "main_track_insert_assets",
  "main_track_list",
  "main_track_move",
  "main_track_trim",
  "main_track_split",
  "main_track_remove",
  "canvas_get",
  "canvas_set",
  "output_rate_get",
  "output_rate_set",
  "subtitle_style_get",
  "subtitle_style_set",
  "apply_default_subtitle_style",
  "speaker_list",
  "speaker_name_proposals",
  "speaker_agent_payload_preview",
  "speaker_name_confirm",
  "speaker_merge_confirm",
  "speaker_diarize_start",
  "speaker_diarization_get",
]);

const moduleDirectory = dirname(fileURLToPath(import.meta.url));
const rendererHtml = resolve(moduleDirectory, "../renderer/index.html");
const preloadPath = resolve(moduleDirectory, "../preload/index.cjs");
const repositoryRoot = resolve(moduleDirectory, "../../..");
const e2eUserData =
  app.isPackaged && !app.commandLine.hasSwitch(E2E_SWITCH)
    ? undefined
    : app.commandLine.getSwitchValue(E2E_USER_DATA_SWITCH);
const userDataPath =
  e2eUserData || join(app.getPath("appData"), "space.ahua.doublelove.studio");

// Preserve the established identifier-based Application Support location before
// any session, window, store, or host is created. E2E supplies an isolated override.
app.setPath("userData", userDataPath);

interface PendingRequest {
  resolve: (response: HostResponse) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout | null;
}

interface HostEventFrame {
  v: 1;
  event: string;
  payload: unknown;
}

function isHostEventFrame(value: unknown): value is HostEventFrame {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as { v?: unknown }).v === PROTOCOL_VERSION &&
    typeof (value as { event?: unknown }).event === "string" &&
    !("id" in value) &&
    "payload" in value
  );
}

class HostSupervisor {
  private child: ChildProcessWithoutNullStreams | null = null;
  private buffer = Buffer.alloc(0);
  private expectedFrameLength: number | null = null;
  private pending = new Map<string, PendingRequest>();
  private healthy = false;
  private stopping = false;
  private capabilities: readonly string[] = [];
  private readonly validateResponse: ValidateFunction<HostResponse>;

  constructor(
    private readonly log: LocalLog,
    private readonly broadcastEvent: (event: string, payload: unknown) => void,
  ) {
    const schemaRoot = app.isPackaged
      ? join(process.resourcesPath, "bindings/host-protocol/schema")
      : join(repositoryRoot, "bindings/host-protocol/schema");
    let responseSchema: object;
    try {
      responseSchema = JSON.parse(
        readFileSync(join(schemaRoot, "HostResponse.schema.json"), "utf8"),
      ) as object;
    } catch (error) {
      throw new Error("Host response schema could not be loaded", {
        cause: error,
      });
    }
    const ajv = new Ajv2020({ allErrors: true, strict: true });
    ajv.addFormat("uint32", {
      type: "number",
      validate: (value: number) =>
        Number.isInteger(value) && value >= 0 && value <= 0xffffffff,
    });
    this.validateResponse = ajv.compile<HostResponse>(responseSchema);
  }

  async start(): Promise<void> {
    if (this.child) return;

    const hostPath = app.isPackaged
      ? join(process.resourcesPath, "double-love-desktop-host")
      : join(repositoryRoot, "target/debug/double-love-desktop-host");

    this.log.write({
      level: "info",
      process: "host",
      method: "lifecycle.start",
      status: "start",
    });
    const startedAt = performance.now();
    this.stopping = false;
    const hostArguments = [
      "--app-data-dir",
      app.getPath("userData"),
      "--resource-dir",
      app.isPackaged ? process.resourcesPath : join(repositoryRoot, "studio/build"),
    ];
    if (
      !app.isPackaged &&
      app.commandLine.hasSwitch(E2E_TRANSCRIBE_MOCK_SWITCH)
    ) {
      hostArguments.push("--test-transcribe-mock");
    }
    if (!app.isPackaged && app.commandLine.hasSwitch(E2E_SPEAKER_MOCK_SWITCH)) {
      hostArguments.push("--test-speaker-mock");
    }
    const child = spawn(hostPath, hostArguments, {
      shell: false,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child = child;
    child.stdout.on("data", (chunk: Buffer) => this.onStdout(chunk));
    child.stderr.resume();
    child.once("error", (error) => this.markUnhealthy(error));
    child.once("exit", (code, signal) => {
      const wasStopping = this.stopping;
      const detail = wasStopping
        ? "Desktop host stopped"
        : `Desktop host exited unexpectedly (${code ?? signal ?? "unknown"})`;
      this.markUnhealthy(new Error(detail));
      this.child = null;
      if (!wasStopping) {
        this.log.writeCrashMarker(code, signal);
        this.log.write({
          level: "error",
          process: "host",
          method: "lifecycle.exit",
          status: "crash",
          errorCode: "HOST_EXIT",
        });
      }
    });

    const response = await this.request({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      method: "handshake",
      client: "electron-main",
      client_protocol: PROTOCOL_VERSION,
    });
    const hello = assertCompatibleHostHello(response, PROTOCOL_VERSION);
    this.capabilities = hello.capabilities;
    this.healthy = true;
    this.log.clearCrashMarker();
    this.log.write({
      level: "info",
      process: "host",
      method: "handshake",
      durationMs: performance.now() - startedAt,
      status: "ok",
      hostVersion: hello.hostVersion,
      engineVersion: hello.engineVersion,
    });
  }

  health(): Promise<HostResponse> {
    if (!this.healthy || !this.capabilities.includes("health")) {
      return Promise.reject(new Error("Desktop host is unhealthy"));
    }
    return this.request({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      method: "health",
    });
  }

  invoke(name: string, payload: unknown): Promise<HostResponse> {
    if (!this.healthy || !this.capabilities.includes("invoke")) {
      return Promise.reject(
        new Error("Desktop host invoke capability is unavailable"),
      );
    }
    return this.request(
      {
        v: PROTOCOL_VERSION,
        id: randomUUID(),
        method: "invoke",
        name,
        payload,
      },
      name === "project_render_mp4_apply"
        ? null
        : name === "import_media"
          ? 30_000
          : REQUEST_TIMEOUT_MS,
    );
  }

  async stop(): Promise<void> {
    const child = this.child;
    if (!child) return;

    this.stopping = true;
    if (this.healthy) {
      try {
        await this.invoke("project_checkpoint", {});
      } catch {
        // WAL remains crash-safe; shutdown still proceeds when the final checkpoint fails.
      }
      try {
        await Promise.race([
          (async () => {
            await this.request({
              v: PROTOCOL_VERSION,
              id: randomUUID(),
              method: "shutdown",
            });
            if (child.exitCode === null && child.signalCode === null)
              await once(child, "exit");
          })(),
          new Promise<never>((_, reject) => {
            setTimeout(
              () => reject(new Error("Desktop host shutdown timed out")),
              SHUTDOWN_TIMEOUT_MS,
            );
          }),
        ]);
      } catch {
        // The hard kill below is the required fallback for failed shutdown.
      }
    }

    if (child.exitCode === null && child.signalCode === null) child.kill();
    this.healthy = false;
    this.log.write({
      level: "info",
      process: "host",
      method: "lifecycle.stop",
      status: "stop",
    });
  }

  stopImmediately(): void {
    const child = this.child;
    this.stopping = true;
    this.healthy = false;
    if (child && child.exitCode === null && child.signalCode === null)
      child.kill();
    this.log.write({
      level: "info",
      process: "host",
      method: "lifecycle.stop",
      status: "stop",
    });
  }

  private request(
    request: HostRequest,
    timeoutMs: number | null = REQUEST_TIMEOUT_MS,
  ): Promise<HostResponse> {
    const child = this.child;
    if (!child?.stdin.writable)
      return Promise.reject(new Error("Desktop host is not running"));

    const payload = Buffer.from(JSON.stringify(request), "utf8");
    if (payload.byteLength > MAX_FRAME_BYTES) {
      return Promise.reject(
        new Error("Desktop host request exceeds the frame limit"),
      );
    }
    const header = Buffer.allocUnsafe(4);
    header.writeUInt32BE(payload.byteLength);

    return new Promise<HostResponse>((resolveRequest, rejectRequest) => {
      const timer =
        timeoutMs === null
          ? null
          : setTimeout(() => {
              this.pending.delete(request.id);
              rejectRequest(
                new Error(`Desktop host request timed out: ${request.method}`),
              );
            }, timeoutMs);
      this.pending.set(request.id, {
        resolve: resolveRequest,
        reject: rejectRequest,
        timer,
      });
      child.stdin.write(Buffer.concat([header, payload]), (error) => {
        if (!error) return;
        const pending = this.pending.get(request.id);
        if (!pending) return;
        if (pending.timer) clearTimeout(pending.timer);
        this.pending.delete(request.id);
        pending.reject(error);
      });
    });
  }

  private onStdout(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);

    while (true) {
      if (this.expectedFrameLength === null) {
        if (this.buffer.byteLength < 4) return;
        this.expectedFrameLength = this.buffer.readUInt32BE(0);
        this.buffer = this.buffer.subarray(4);
        if (this.expectedFrameLength > MAX_FRAME_BYTES) {
          this.markUnhealthy(
            new Error("Desktop host response exceeds the frame limit"),
          );
          this.child?.kill();
          return;
        }
      }

      if (this.buffer.byteLength < this.expectedFrameLength) return;
      const frame = this.buffer.subarray(0, this.expectedFrameLength);
      this.buffer = this.buffer.subarray(this.expectedFrameLength);
      this.expectedFrameLength = null;
      this.handleFrame(frame);
    }
  }

  private handleFrame(frame: Buffer): void {
    let value: unknown;
    try {
      value = JSON.parse(frame.toString("utf8"));
    } catch {
      this.markUnhealthy(new Error("Desktop host returned invalid JSON"));
      this.child?.kill();
      return;
    }

    if (isHostEventFrame(value)) {
      this.broadcastEvent(value.event, value.payload);
      return;
    }

    if (!this.validateResponse(value)) {
      this.markUnhealthy(
        new Error("Desktop host response failed protocol schema validation"),
      );
      this.child?.kill();
      return;
    }

    const pending = this.pending.get(value.id);
    if (!pending) return;
    if (pending.timer) clearTimeout(pending.timer);
    this.pending.delete(value.id);
    pending.resolve(value);
  }

  private markUnhealthy(error: Error): void {
    this.healthy = false;
    this.capabilities = [];
    for (const pending of this.pending.values()) {
      if (pending.timer) clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
}

let mainWindow: BrowserWindow | null = null;
let settingsWindow: BrowserWindow | null = null;
let host: HostSupervisor | null = null;
let quitReady: (() => void) | null = null;
const quitFlow = createQuitFlowState();

const grants = new PathGrants();
const log = new LocalLog(app.getPath("userData"));
const updater = new UpdaterController({
  updater: autoUpdater,
  log,
  broadcast: broadcastRendererEvent,
  isPackaged: app.isPackaged,
  e2eEnabled: app.commandLine.hasSwitch(E2E_SWITCH),
  feedUrl: process.env.DOUBLELOVE_UPDATE_FEED_URL,
  feedConfigPath: join(app.getPath("userData"), "e2e-app-update.yml"),
  setInstalling: (installing) => {
    quitFlow.installingUpdate = installing;
  },
});
const usePackagedRenderer =
  app.isPackaged || app.commandLine.hasSwitch(E2E_SWITCH);
const e2eMode = app.commandLine.hasSwitch(E2E_SWITCH);
const allowE2eDialogOverride =
  !app.isPackaged && app.commandLine.hasSwitch(E2E_SWITCH);
const developmentRendererUrl = "http://localhost:5174";

function isExpectedNavigation(target: string): boolean {
  try {
    const url = new URL(target);
    if (!usePackagedRenderer)
      return url.origin === new URL(developmentRendererUrl).origin;
    return (
      url.protocol === "dl-app:" &&
      url.hostname === "app" &&
      url.pathname === "/index.html"
    );
  } catch {
    return false;
  }
}

function secureWindow(window: BrowserWindow): void {
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-navigate", (event, target) => {
    if (!isExpectedNavigation(target)) event.preventDefault();
  });
  window.webContents.on("before-input-event", (event, input) => {
    if (
      input.type.toLowerCase().includes("keydown") &&
      input.meta &&
      input.key === ","
    ) {
      event.preventDefault();
      openSettings();
    }
  });
}

function windowOptions(): Electron.BrowserWindowConstructorOptions {
  return {
    title: "Double Love Studio",
    // E2E owns the window lifecycle. Keep automation out of the user's desktop
    // while preserving a real BrowserWindow/WebContents for Playwright.
    show: !e2eMode,
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
    ...(process.platform === "darwin"
      ? { trafficLightPosition: { x: 14, y: 16 } }
      : {}),
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      webviewTag: false,
    },
  };
}

async function loadRenderer(
  window: BrowserWindow,
  settings = false,
): Promise<void> {
  if (!usePackagedRenderer) {
    const suffix = settings ? `?${SETTINGS_QUERY}` : "";
    await window.loadURL(`${developmentRendererUrl}${suffix}`);
    return;
  }
  const suffix = settings ? `?${SETTINGS_QUERY}` : "";
  await window.loadURL(`${RENDERER_ORIGIN}/index.html${suffix}`);
}

function installRendererProtocol(): void {
  const rendererRoot = dirname(rendererHtml);
  protocol.handle("dl-app", (request) => {
    try {
      const url = new URL(request.url);
      if (url.hostname !== "app") return new Response(null, { status: 404 });
      const candidate = resolve(
        rendererRoot,
        `.${decodeURIComponent(url.pathname)}`,
      );
      const containedPath = relative(rendererRoot, candidate);
      if (
        containedPath === ".." ||
        containedPath.startsWith(`..${sep}`) ||
        isAbsolute(containedPath)
      ) {
        return new Response(null, { status: 404 });
      }
      return net
        .fetch(pathToFileURL(candidate).toString())
        .catch(() => new Response(null, { status: 404 }));
    } catch {
      return new Response(null, { status: 404 });
    }
  });
}

function createMainWindow(): BrowserWindow {
  const window = new BrowserWindow({
    ...windowOptions(),
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
  });
  secureWindow(window);
  window.webContents.once("did-finish-load", () => {
    try {
      loadNativePlayer().attach(window.getNativeWindowHandle());
      if (playerPoll) clearInterval(playerPoll);
      playerPoll = setInterval(() => {
        if (!window.isDestroyed() && nativePlayer) {
          window.webContents.send("dl:player-state", nativePlayer.state());
        }
      }, 100);
    } catch {
      log.write({ level: "error", process: "main", method: "player.attach", status: "error", errorCode: "AVFOUNDATION_PLAYER_FAILED" });
    }
  });
  window.once("closed", () => {
    if (playerPoll) clearInterval(playerPoll);
    playerPoll = null;
    nativePlayer?.dispose();
    mainWindow = null;
  });
  void loadRenderer(window).then(() => updater.checkOnStartup());
  return window;
}

function openSettings(): void {
  if (settingsWindow && !settingsWindow.isDestroyed()) {
    settingsWindow.show();
    settingsWindow.focus();
    return;
  }

  const window = new BrowserWindow({
    ...windowOptions(),
    width: 760,
    height: 580,
    minWidth: 700,
    minHeight: 500,
  });
  settingsWindow = window;
  secureWindow(window);
  window.on("close", (event) => {
    if (quitFlow.allowQuit || quitFlow.installingUpdate) return;
    event.preventDefault();
    window.hide();
  });
  window.once("closed", () => {
    settingsWindow = null;
  });
  void loadRenderer(window, true);
}

function installMenu(): void {
  const template: MenuItemConstructorOptions[] = [
    {
      label: "Double Love Studio",
      submenu: [
        {
          id: "settings",
          label: "设置…",
          accelerator: "Cmd+,",
          click: openSettings,
        },
        { type: "separator" },
        { role: "quit" },
      ],
    },
    {
      label: "文件",
      submenu: [
        {
          label: "新建项目",
          accelerator: "Cmd+N",
          click: () =>
            broadcastRendererEvent("dl://menu-command", {
              command: "new-project",
            }),
        },
        {
          label: "导入已有项目…",
          accelerator: "Cmd+O",
          click: () =>
            broadcastRendererEvent("dl://menu-command", {
              command: "import-project",
            }),
        },
        { type: "separator" },
        { role: "close" },
      ],
    },
    {
      label: "编辑",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "selectAll" },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

function roleForSender(event: IpcMainInvokeEvent): WindowRole | null {
  if (
    mainWindow &&
    !mainWindow.isDestroyed() &&
    event.sender === mainWindow.webContents
  )
    return "main";
  if (
    settingsWindow &&
    !settingsWindow.isDestroyed() &&
    event.sender === settingsWindow.webContents
  ) {
    return "settings";
  }
  return null;
}

function guardOptions(
  allowedWindows: readonly WindowRole[] = BOTH_WINDOWS,
): GuardedIpcOptions {
  return {
    allowedWindows,
    expectedUrl: isExpectedNavigation,
    roleForSender,
    onOutcome: (outcome) =>
      log.write({
        level: outcome.status === "ok" ? "info" : "warn",
        process: "main",
        method: outcome.channel,
        durationMs: outcome.durationMs,
        status: outcome.status,
        ...(outcome.errorCode ? { errorCode: outcome.errorCode } : {}),
      }),
  };
}

function parentForEvent(event: IpcMainInvokeEvent): BrowserWindow {
  const parent = BrowserWindow.fromWebContents(event.sender);
  if (!parent)
    throw new Error("IPC_FORBIDDEN: Dialog sender has no application window");
  return parent;
}

function e2ePathFrom(payload: Record<string, unknown>): string | null {
  if (!allowE2eDialogOverride || typeof payload.e2ePath !== "string")
    return null;
  if (!isAbsolute(payload.e2ePath) || payload.e2ePath.length === 0) {
    throw new Error("INVALID_PARAMS: e2ePath must be an absolute path");
  }
  return payload.e2ePath;
}

function grantResponse(
  path: string,
  kind: GrantKind,
  includeDisplayName = false,
): { token: string; displayName?: string } {
  return grants.create(path, kind, includeDisplayName);
}

function invalidHostResponse(code: string, message: string): HostResponse {
  return {
    v: PROTOCOL_VERSION,
    id: randomUUID(),
    status: "error",
    error: { code, message },
  };
}

function failedInvokeResponse(code: string, message: string, suggestedAction?: string): HostResponse {
  return {
    v: PROTOCOL_VERSION,
    id: randomUUID(),
    status: "ok",
    result: {
      type: "invoke",
      data: {
        status: "failed",
        revision: null,
        data: null,
        diagnostics: [{
          level: "error",
          code,
          cause: message,
          object_id: null,
          impact: "素材没有导入项目。",
          blocks_export: true,
          suggested_action: suggestedAction ?? null,
        }],
        outputs: [],
        counts: { total: 0, processed: 0, skipped: 0, failed: 1, unmatched: 0 },
      },
    },
  };
}

function invokeOperationSucceeded(response: HostResponse): boolean {
  if (response.status !== "ok" || response.result.type !== "invoke") return false;
  const data = response.result.data;
  return typeof data === "object" && data !== null && (data as { status?: unknown }).status === "success";
}

function installIpcHandlers(): void {
  registerGuardedHandler("dl:host-health", guardOptions(), () =>
    host?.health(),
  );
  registerGuardedHandler("app:open-settings", guardOptions(), () =>
    openSettings(),
  );
  registerGuardedHandler("app:get-info", guardOptions(), () => ({
    name: app.getName(),
    version: app.getVersion(),
  }));
  registerGuardedHandler("player:set-bounds", guardOptions(["main"]), (_event, value) => {
    const bounds = value as { x?: number; y?: number; width?: number; height?: number };
    if (![bounds.x, bounds.y, bounds.width, bounds.height].every(Number.isFinite)) {
      throw new Error("INVALID_PARAMS: Player bounds are invalid");
    }
    loadNativePlayer().setFrame(bounds.x!, bounds.y!, bounds.width!, bounds.height!);
    return null;
  });
  registerGuardedHandler("player:load-timeline", guardOptions(["main"]), async (_event, value) => {
    const payload = value as {
      clips?: Array<{
        assetId?: unknown;
        sourceStartSeconds?: unknown;
        sourceDurationSeconds?: unknown;
        outputStartSeconds?: unknown;
        outputDurationSeconds?: unknown;
      }>;
      seconds?: unknown;
    };
    if (!Array.isArray(payload.clips) || payload.clips.length === 0 || payload.clips.length > 10_000 || typeof payload.seconds !== "number") {
      throw new Error("INVALID_PARAMS: Player timeline is invalid");
    }
    const clips = await Promise.all(payload.clips.map(async (clip) => {
      if (typeof clip.assetId !== "string" || ![
        clip.sourceStartSeconds,
        clip.sourceDurationSeconds,
        clip.outputStartSeconds,
        clip.outputDurationSeconds,
      ].every((item) => typeof item === "number" && Number.isFinite(item) && item >= 0)) {
        throw new Error("INVALID_PARAMS: Player clip is invalid");
      }
      const response = await host?.invoke("resolve_media_asset", { asset_id: clip.assetId });
      const path = response ? extractResolvedPath(response) : null;
      if (!path) throw new Error("MEDIA_ASSET_NOT_FOUND: Player source is unavailable");
      return {
        path,
        sourceStartSeconds: clip.sourceStartSeconds,
        sourceDurationSeconds: clip.sourceDurationSeconds,
        outputStartSeconds: clip.outputStartSeconds,
        outputDurationSeconds: clip.outputDurationSeconds,
      };
    }));
    loadNativePlayer().loadTimeline(JSON.stringify(clips), payload.seconds);
    return null;
  });
  registerGuardedHandler("player:set-subtitle", guardOptions(["main"]), (_event, value) => {
    if (typeof value !== "object" || value === null) throw new Error("INVALID_PARAMS: Player subtitle is invalid");
    loadNativePlayer().setSubtitle(JSON.stringify(value));
    return null;
  });
  registerGuardedHandler("player:set-presentation", guardOptions(["main"]), (_event, value) => {
    if (typeof value !== "object" || value === null) throw new Error("INVALID_PARAMS: Player presentation is invalid");
    loadNativePlayer().setPresentation(JSON.stringify(value));
    return null;
  });
  registerGuardedHandler("player:play", guardOptions(["main"]), () => { loadNativePlayer().play(); return null; });
  registerGuardedHandler("player:pause", guardOptions(["main"]), () => { loadNativePlayer().pause(); return null; });
  registerGuardedHandler("player:seek", guardOptions(["main"]), (_event, value) => {
    const seconds = (value as { seconds?: unknown })?.seconds;
    if (typeof seconds !== "number" || !Number.isFinite(seconds)) throw new Error("INVALID_PARAMS: Player time is invalid");
    loadNativePlayer().seek(seconds); return null;
  });
  registerGuardedHandler("player:dispose", guardOptions(["main"]), () => { nativePlayer?.dispose(); return null; });
  registerGuardedHandler("project:quit-ready", guardOptions(["main"]), () => {
    quitReady?.();
    quitReady = null;
    return null;
  });
  registerGuardedHandler("update:check", guardOptions(), () =>
    updater.checkManually(),
  );
  registerGuardedHandler("update:download", guardOptions(), () =>
    updater.download(),
  );
  registerGuardedHandler("update:install", guardOptions(), () =>
    updater.install(),
  );

  registerGuardedHandler(
    "project:create",
    guardOptions(["main"]),
    async (_event, value) => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        return invalidHostResponse(
          "INVALID_PARAMS",
          "Project creation options are required",
        );
      }
      const payload = value as Record<string, unknown>;
      let customParent: string | undefined;
      if (payload.parentGrantToken !== undefined) {
        if (!allowE2eDialogOverride) {
          return invalidHostResponse(
            "PROJECT_LOCATION_FIXED",
            "项目必须保存在默认位置。",
          );
        }
        const resolvedParent = grants.resolve(
          payload.parentGrantToken,
          "project-parent",
        );
        if (isInvalidGrant(resolvedParent))
          return invalidHostResponse(
            resolvedParent.code,
            resolvedParent.message,
          );
        customParent = resolvedParent;
      }
      const target = resolveProjectTarget({
        name: payload.name,
        moviesDirectory: app.getPath("videos"),
        customParent,
      });
      if (!target.ok) return invalidHostResponse(target.code, target.message);
      if (payload.parentGrantToken !== undefined) {
        const consumed = grants.consume(
          payload.parentGrantToken,
          "project-parent",
        );
        if (isInvalidGrant(consumed))
          return invalidHostResponse(consumed.code, consumed.message);
      }
      const response = await host?.invoke("project_create", {
        path: target.target,
      });
      return (
        response ??
        invalidHostResponse("HOST_UNAVAILABLE", "Desktop host is unavailable")
      );
    },
  );

  registerGuardedHandler(
    "project:trash",
    guardOptions(["main"]),
    async (_event, projectId) => {
      if (typeof projectId !== "string" || !/^[A-Za-z0-9._-]{1,128}$/u.test(projectId)) {
        return invalidHostResponse("INVALID_PARAMS", "Project id is invalid");
      }
      const prepared = await host?.invoke("prepare_project_trash", {
        project_id: projectId,
      });
      if (!prepared)
        return invalidHostResponse("HOST_UNAVAILABLE", "Desktop host is unavailable");
      const path = extractResolvedPath(prepared);
      const wasCurrent = extractWasCurrent(prepared);
      if (!path) return prepared;
      const resolvedPath = resolve(path);
      const protectedDirectories = [
        app.getPath("home"),
        app.getPath("desktop"),
        app.getPath("documents"),
        app.getPath("downloads"),
        app.getPath("videos"),
      ].map((value) => resolve(value));
      if (resolvedPath === resolve("/") || protectedDirectories.includes(resolvedPath)) {
        return invalidHostResponse(
          "PROJECT_TRASH_FORBIDDEN",
          "这个项目位于系统常用目录，不能整体移到废纸篓。请先把项目迁移到独立文件夹。",
        );
      }
      try {
        await shell.trashItem(resolvedPath);
      } catch {
        return invalidHostResponse(
          "PROJECT_TRASH_FAILED",
          "项目没有移到废纸篓，请确认文件夹仍可访问。",
        );
      }
      if (wasCurrent) await host?.invoke("project_close", {});
      const forgotten = await host?.invoke("recent_project_forget", { root: path });
      return (
        forgotten ??
        invalidHostResponse("HOST_UNAVAILABLE", "Desktop host is unavailable")
      );
    },
  );

  registerGuardedHandler(
    "dl:dialog-pick-directory",
    guardOptions(),
    async (event, value) => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error(
          "INVALID_PARAMS: Directory dialog options are required",
        );
      }
      const payload = value as Record<string, unknown>;
      const kind = payload.kind;
      if (
        kind !== "project-open" &&
        kind !== "project-parent" &&
        kind !== "model-root" &&
        kind !== "model-import"
      ) {
        throw new Error("INVALID_PARAMS: Unsupported directory grant kind");
      }
      if (kind === "project-parent" && !allowE2eDialogOverride) {
        throw new Error("PROJECT_LOCATION_FIXED: Project location is fixed");
      }
      if (
        typeof payload.title !== "string" ||
        payload.title.length === 0 ||
        payload.title.length > 200
      ) {
        throw new Error("INVALID_PARAMS: Dialog title is invalid");
      }
      const override = e2ePathFrom(payload);
      if (override)
        return grantResponse(override, kind, kind === "project-parent");

      const result = await dialog.showOpenDialog(parentForEvent(event), {
        title: payload.title,
        properties: ["openDirectory"],
      });
      return result.canceled || result.filePaths.length === 0
        ? null
        : grantResponse(result.filePaths[0], kind, kind === "project-parent");
    },
  );

  registerGuardedHandler(
    "dl:dialog-pick-media-file",
    guardOptions(),
    async (event, value) => {
      const payload =
        typeof value === "object" && value !== null && !Array.isArray(value)
          ? (value as Record<string, unknown>)
          : {};
      const override = e2ePathFrom(payload);
      if (override) return grantResponse(override, "import-media");

      const result = await dialog.showOpenDialog(parentForEvent(event), {
        title: "选择要导入的媒体文件",
        properties: ["openFile"],
        filters: [{ name: "视频", extensions: ["mp4", "mov", "m4v", "mxf"] }],
      });
      return result.canceled || result.filePaths.length === 0
        ? null
        : grantResponse(result.filePaths[0], "import-media");
    },
  );

  registerGuardedHandler(
    "dl:grant-dropped-media",
    guardOptions(["main"]),
    (_event, value) => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error("INVALID_PARAMS: Dropped media paths are required");
      }
      const paths = (value as { paths?: unknown }).paths;
      if (!Array.isArray(paths) || paths.length === 0 || paths.length > 64) {
        throw new Error("INVALID_PARAMS: Dropped media paths are invalid");
      }
      return paths.map((path) => {
        if (typeof path !== "string" || !isAbsolute(path) || !statSync(path).isFile()) {
          throw new Error("INVALID_PARAMS: Dropped media must be normal files");
        }
        return grantResponse(path, "import-media", true);
      });
    },
  );

  registerGuardedHandler(
    "dl:dialog-pick-export-path",
    guardOptions(),
    async (event, value) => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error("INVALID_PARAMS: Export dialog options are required");
      }
      const payload = value as Record<string, unknown>;
      const kind = payload.kind;
      if (kind !== "xml" && kind !== "ass" && kind !== "mp4") {
        throw new Error("INVALID_PARAMS: Unsupported export kind");
      }
      if (
        typeof payload.defaultName !== "string" ||
        payload.defaultName.length === 0 ||
        payload.defaultName.length > 255 ||
        basename(payload.defaultName) !== payload.defaultName
      ) {
        throw new Error("INVALID_PARAMS: Export defaultName is invalid");
      }
      const override = e2ePathFrom(payload);
      if (override) return grantResponse(override, "export-save");

      const exportFilterName = {
        xml: "Premiere / Resolve XML",
        ass: "ASS 字幕",
        mp4: "带字幕 MP4",
      }[kind];
      const result = await dialog.showSaveDialog(parentForEvent(event), {
        title: `导出 ${exportFilterName}`,
        defaultPath: payload.defaultName,
        filters: [{ name: exportFilterName, extensions: [kind] }],
      });
      return result.canceled || !result.filePath
        ? null
        : grantResponse(result.filePath, "export-save");
    },
  );

  registerGuardedHandler(
    "dl:invoke",
    guardOptions(),
    async (_event, name, payload) => {
      if (typeof name !== "string" || !/^[a-z][a-z0-9_]{0,127}$/u.test(name)) {
        return invalidHostResponse("INVALID_PARAMS", "Command name is invalid");
      }
      if (!RENDERER_COMMANDS.has(name)) {
        return invalidHostResponse(
          "IPC_FORBIDDEN",
          "Command is not exposed to the renderer",
        );
      }

      const startedAt = performance.now();
      const granted = applyGrantPolicy(grants, name, payload ?? {});
      if (!granted.ok) {
        log.write({
          level: "warn",
          process: "main",
          method: "ipc.invoke",
          durationMs: performance.now() - startedAt,
          status: "error",
          errorCode: granted.error.code,
        });
        return invalidHostResponse(granted.error.code, granted.error.message);
      }

      try {
        if (name === "import_media") {
          const path = typeof granted.payload === "object" && granted.payload !== null
            ? (granted.payload as { path?: unknown }).path
            : null;
          if (typeof path !== "string") {
            return invalidHostResponse("INVALID_PARAMS", "Media path is invalid");
          }
          const preflight = await host?.invoke("media_preflight", { path });
          if (!preflight) {
            return invalidHostResponse("HOST_UNAVAILABLE", "Desktop host is unavailable");
          }
          if (!invokeOperationSucceeded(preflight)) return preflight;
          if (/\.webm$/iu.test(path) || !loadNativePlayer().isPlayable(path)) {
            return failedInvokeResponse(
              "MEDIA_PLAYBACK_UNSUPPORTED",
              "这个文件不能由 macOS AVFoundation 播放。当前支持 H.264、HEVC、ProRes、ProRes RAW 以及 AVFoundation 可识别的 MXF。",
              "请转换为 macOS 可直接播放的 MP4、MOV 或 MXF 后重试。",
            );
          }
        }
        const hostPayload =
          name === "doctor_run"
            ? {
                ...(typeof granted.payload === "object" && granted.payload !== null
                  ? granted.payload
                  : {}),
                app_version: app.getVersion(),
              }
            : name === "model_install" || name === "model_resume"
              ? {
                  ...(typeof granted.payload === "object" && granted.payload !== null
                    ? granted.payload
                    : {}),
                  app_version: app.getVersion(),
                }
              : granted.payload;
        const response = await host?.invoke(name, hostPayload);
        if (!response)
          return invalidHostResponse(
            "HOST_UNAVAILABLE",
            "Desktop host is unavailable",
          );
        let rendererResponse = response;
        if (name === "model_reveal" || name === "diagnostics_reveal_logs") {
          const path = extractResolvedPath(response);
          if (path && !e2eMode) {
            if (name === "model_reveal") {
              shell.showItemInFolder(path);
            } else {
              const openError = await shell.openPath(path);
              if (openError) {
                return invalidHostResponse(
                  "LOG_REVEAL_FAILED",
                  "The requested application directory could not be opened",
                );
              }
            }
          }
          rendererResponse = sanitizeRevealResponse(response);
        }
        log.write({
          level: response.status === "ok" ? "info" : "warn",
          process: "main",
          requestId: response.id,
          method: "ipc.invoke",
          durationMs: performance.now() - startedAt,
          status: response.status,
          ...(response.status === "error"
            ? { errorCode: response.error.code }
            : {}),
        });
        return rendererResponse;
      } catch {
        log.write({
          level: "error",
          process: "main",
          method: "ipc.invoke",
          durationMs: performance.now() - startedAt,
          status: "error",
          errorCode: "HOST_UNAVAILABLE",
        });
        return invalidHostResponse(
          "HOST_UNAVAILABLE",
          "Desktop host is unavailable",
        );
      }
    },
  );
}

function broadcastRendererEvent(event: string, payload: unknown): void {
  for (const window of [mainWindow, settingsWindow]) {
    if (window && !window.isDestroyed())
      window.webContents.send("dl:host-event", event, payload);
  }
}

function extractResolvedPath(response: HostResponse): string | null {
  if (response.status !== "ok" || response.result.type !== "invoke")
    return null;
  const data = response.result.data;
  if (typeof data === "string") return data;
  if (typeof data !== "object" || data === null) return null;
  const record = data as Record<string, unknown>;
  if (typeof record.path === "string") return record.path;
  if (typeof record.data === "string") return record.data;
  if (typeof record.data === "object" && record.data !== null) {
    const nested = record.data as Record<string, unknown>;
    if (typeof nested.path === "string") return nested.path;
  }
  return null;
}

function extractWasCurrent(response: HostResponse): boolean {
  if (response.status !== "ok" || response.result.type !== "invoke") return false;
  const operation = response.result.data;
  if (typeof operation !== "object" || operation === null || Array.isArray(operation))
    return false;
  const data = (operation as Record<string, unknown>).data;
  return typeof data === "object" && data !== null && !Array.isArray(data)
    ? (data as Record<string, unknown>).was_current === true
    : false;
}

function sanitizeRevealResponse(response: HostResponse): HostResponse {
  if (
    response.status !== "ok" ||
    response.result.type !== "invoke" ||
    typeof response.result.data !== "object" ||
    response.result.data === null ||
    Array.isArray(response.result.data)
  ) {
    return response;
  }
  const operation: Record<string, unknown> = {
    ...(response.result.data as Record<string, unknown>),
    data: null,
  };
  delete operation.path;
  return {
    ...response,
    result: {
      ...response.result,
      data: operation,
    },
  };
}

function installMediaProtocol(): void {
  protocol.handle("dl-media", async (request) => {
    let assetId: string;
    try {
      const url = new URL(request.url);
      const segments = url.pathname.split("/").filter(Boolean);
      if (url.hostname !== "asset" || segments.length !== 1)
        return mediaNotFound();
      assetId = decodeURIComponent(segments[0]);
      if (assetId.length === 0) return mediaNotFound();
    } catch {
      return mediaNotFound();
    }

    try {
      const response = await host?.invoke("resolve_media_asset", {
        asset_id: assetId,
      });
      if (!response) return mediaNotFound();
      const path = extractResolvedPath(response);
      if (!path) {
        log.write({
          level: "warn",
          process: "protocol",
          method: "resolve_media_asset",
          status: "error",
          ...(response.status === "error"
            ? { errorCode: response.error.code }
            : {}),
        });
        return mediaNotFound();
      }
      return mediaResponse(request.method, request.headers.get("range"), path);
    } catch {
      log.write({
        level: "error",
        process: "protocol",
        method: "resolve_media_asset",
        status: "error",
        errorCode: "PROTOCOL_ERROR",
      });
      return mediaNotFound();
    }
  });
}

function installProjectThumbnailProtocol(): void {
  protocol.handle("dl-thumbnail", async (request) => {
    const projectId = projectIdFromThumbnailUrl(request.url);
    if (!projectId) return mediaNotFound();

    try {
      const response = await host?.invoke("resolve_project_thumbnail", {
        project_id: projectId,
      });
      if (!response) return mediaNotFound();
      const path = extractResolvedPath(response);
      if (!path) return mediaNotFound();
      return mediaResponse(request.method, request.headers.get("range"), path);
    } catch {
      log.write({
        level: "error",
        process: "protocol",
        method: "resolve_project_thumbnail",
        status: "error",
        errorCode: "PROTOCOL_ERROR",
      });
      return mediaNotFound();
    }
  });
}

const hasSingleInstanceLock = app.requestSingleInstanceLock();
if (!hasSingleInstanceLock) {
  app.quit();
} else {
  app.on("second-instance", () => {
    if (!mainWindow) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.show();
    mainWindow.focus();
  });

  app
    .whenReady()
    .then(async () => {
      if (e2eMode && process.platform === "darwin") app.dock?.hide();
      session.defaultSession.setPermissionRequestHandler(
        (_webContents, _permission, callback) => {
          callback(false);
        },
      );

      session.defaultSession.webRequest.onHeadersReceived(
        (details, callback) => {
          const csp = usePackagedRenderer
            ? "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: dl-thumbnail:; media-src 'self' blob: dl-media:; font-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
            : "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: dl-thumbnail:; media-src 'self' blob: dl-media:; font-src 'self' data:; connect-src 'self' ws:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
          callback({
            responseHeaders: {
              ...details.responseHeaders,
              "Content-Security-Policy": [csp],
            },
          });
        },
      );

      host = new HostSupervisor(log, broadcastRendererEvent);
      await host.start();
      installMenu();
      installIpcHandlers();
      installRendererProtocol();
      installMediaProtocol();
      installProjectThumbnailProtocol();
      mainWindow = createMainWindow();

      app.on("activate", () => {
        if (!mainWindow) mainWindow = createMainWindow();
        else {
          mainWindow.show();
          mainWindow.focus();
        }
      });
    })
    .catch(() => {
      log.write({
        level: "error",
        process: "main",
        method: "startup",
        status: "error",
        errorCode: "STARTUP_FAILED",
      });
      app.quit();
    });
}

app.on("window-all-closed", () => {
  app.quit();
});

app.on("before-quit", (event) => {
  handleBeforeQuit(event, quitFlow, host, () => app.quit(), () => {
    if (!mainWindow || mainWindow.isDestroyed()) return Promise.resolve();
    return new Promise<void>((resolve) => {
      let finished = false;
      const finish = () => {
        if (finished) return;
        finished = true;
        quitReady = null;
        resolve();
      };
      quitReady = finish;
      mainWindow?.webContents.send("dl:prepare-quit");
      setTimeout(finish, 2_000);
    });
  });
});
