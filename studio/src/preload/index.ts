import { contextBridge, ipcRenderer, webUtils } from "electron";

const ALLOWED_HOST_EVENTS = new Set([
  "dl://progress",
  "dl://task-state",
  "dl://model-progress",
  "dl://model-state",
  "dl://model-queue",
  "dl://preferences-changed",
  "dl://doctor-result",
  "dl://update-status",
  "dl://menu-command",
]);

export type DirectoryGrantKind =
  | "project-open"
  | "project-parent"
  | "model-root"
  | "model-import";
export type ExportKind = "xml" | "ass" | "mp4";

export interface GrantToken {
  token: string;
  displayName?: string;
}

interface E2eOverride {
  e2ePath?: string;
}

const dialogs = Object.freeze({
  pickDirectory: (
    options: { title: string; kind: DirectoryGrantKind } & E2eOverride,
  ): Promise<GrantToken | null> =>
    ipcRenderer.invoke("dl:dialog-pick-directory", options),
  pickMediaFile: (options?: E2eOverride): Promise<GrantToken | null> =>
    ipcRenderer.invoke("dl:dialog-pick-media-file", options),
  grantDroppedMedia: (files: File[]): Promise<GrantToken[]> => {
    const paths = files
      .map((file) => webUtils.getPathForFile(file))
      .filter((path) => path.length > 0);
    return ipcRenderer.invoke("dl:grant-dropped-media", { paths });
  },
  pickExportPath: (
    options: { defaultName: string; kind: ExportKind } & E2eOverride,
  ): Promise<GrantToken | null> =>
    ipcRenderer.invoke("dl:dialog-pick-export-path", options),
});

const updates = Object.freeze({
  check: (): Promise<unknown> => ipcRenderer.invoke("update:check"),
  download: (): Promise<unknown> => ipcRenderer.invoke("update:download"),
  install: (): Promise<unknown> => ipcRenderer.invoke("update:install"),
});

const doubleLove = Object.freeze({
  hostHealth: (): Promise<unknown> => ipcRenderer.invoke("dl:host-health"),
  openSettings: (): Promise<void> => ipcRenderer.invoke("app:open-settings"),
  getAppInfo: (): Promise<unknown> => ipcRenderer.invoke("app:get-info"),
  createProject: (options: {
    name: string;
    parentGrantToken?: string;
  }): Promise<unknown> => ipcRenderer.invoke("project:create", options),
  trashProject: (projectId: string): Promise<unknown> =>
    ipcRenderer.invoke("project:trash", projectId),
  onPrepareQuit: (callback: () => void | Promise<void>): (() => void) => {
    const listener = (): void => {
      void Promise.resolve(callback()).finally(() =>
        ipcRenderer.invoke("project:quit-ready"),
      );
    };
    ipcRenderer.on("dl:prepare-quit", listener);
    return () => ipcRenderer.removeListener("dl:prepare-quit", listener);
  },
  updates,
  dialogs,
  player: Object.freeze({
    setBounds: (bounds: { x: number; y: number; width: number; height: number }) => ipcRenderer.invoke("player:set-bounds", bounds),
    loadTimeline: (clips: Array<{ assetId: string; sourceStartSeconds: number; sourceDurationSeconds: number; outputStartSeconds: number; outputDurationSeconds: number }>, seconds: number) => ipcRenderer.invoke("player:load-timeline", { clips, seconds }),
    setSubtitle: (config: Record<string, string | number>) => ipcRenderer.invoke("player:set-subtitle", config),
    setPresentation: (config: Record<string, string | number>) => ipcRenderer.invoke("player:set-presentation", config),
    play: () => ipcRenderer.invoke("player:play"),
    pause: () => ipcRenderer.invoke("player:pause"),
    seek: (seconds: number) => ipcRenderer.invoke("player:seek", { seconds }),
    dispose: () => ipcRenderer.invoke("player:dispose"),
    onState: (callback: (state: unknown) => void) => {
      const listener = (_event: Electron.IpcRendererEvent, state: unknown) => callback(state);
      ipcRenderer.on("dl:player-state", listener);
      return () => ipcRenderer.removeListener("dl:player-state", listener);
    },
  }),
  invoke: (name: string, payload?: unknown): Promise<unknown> =>
    ipcRenderer.invoke("dl:invoke", name, payload),
  onEvent: (
    channel: string,
    callback: (payload: unknown) => void,
  ): (() => void) => {
    if (!ALLOWED_HOST_EVENTS.has(channel)) {
      throw new TypeError("Unsupported host event channel");
    }
    if (typeof callback !== "function")
      throw new TypeError("Host event callback must be a function");

    const listener = (
      _event: Electron.IpcRendererEvent,
      eventName: string,
      payload: unknown,
    ): void => {
      if (eventName === channel) callback(payload);
    };
    ipcRenderer.on("dl:host-event", listener);
    return () => ipcRenderer.removeListener("dl:host-event", listener);
  },
});

contextBridge.exposeInMainWorld("doubleLove", doubleLove);
