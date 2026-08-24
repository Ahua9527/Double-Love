export interface GrantToken {
  token: string;
  displayName?: string;
}

export interface HostResponse {
  status: "ok" | "error";
  error?: { code: string; message: string };
}

export interface UpdateStatus {
  stage:
    | "idle"
    | "checking-update"
    | "update-available"
    | "update-not-available"
    | "download-progress"
    | "update-downloaded"
    | "error";
  version?: string;
  percent?: number;
  error?: string;
}

export interface DoubleLoveApi {
  hostHealth(): Promise<unknown>;
  openSettings(): Promise<void>;
  getAppInfo(): Promise<{ name: string; version: string }>;
  createProject(options: {
    name: string;
    parentGrantToken?: string;
  }): Promise<HostResponse>;
  trashProject(projectId: string): Promise<HostResponse>;
  onPrepareQuit(callback: () => void | Promise<void>): () => void;
  readonly updates: {
    check(): Promise<UpdateStatus>;
    download(): Promise<UpdateStatus>;
    install(): Promise<UpdateStatus>;
  };
  readonly dialogs: {
    pickDirectory(options: {
      title: string;
      kind: "project-open" | "project-parent" | "model-root" | "model-import";
      e2ePath?: string;
    }): Promise<GrantToken | null>;
    pickMediaFile(options?: { e2ePath?: string }): Promise<GrantToken | null>;
    grantDroppedMedia(files: File[]): Promise<GrantToken[]>;
    pickExportPath(options: {
      defaultName: string;
      kind: "xml" | "ass" | "mp4";
      e2ePath?: string;
    }): Promise<GrantToken | null>;
  };
  readonly player: {
    setBounds(bounds: { x: number; y: number; width: number; height: number }): Promise<void>;
    loadTimeline(clips: Array<{ assetId: string; sourceStartSeconds: number; sourceDurationSeconds: number; outputStartSeconds: number; outputDurationSeconds: number }>, seconds: number): Promise<void>;
    setSubtitle(config: Record<string, string | number>): Promise<void>;
    setPresentation(config: Record<string, string | number>): Promise<void>;
    play(): Promise<void>;
    pause(): Promise<void>;
    seek(seconds: number): Promise<void>;
    dispose(): Promise<void>;
    onState(callback: (state: { state: string; seconds: number; duration: number; rate: number; ready_for_display: boolean; error: string }) => void): () => void;
  };
  invoke(name: string, payload?: unknown): Promise<HostResponse>;
  onEvent(channel: string, callback: (payload: unknown) => void): () => void;
}

declare global {
  interface Window {
    readonly doubleLove: DoubleLoveApi;
  }
}
