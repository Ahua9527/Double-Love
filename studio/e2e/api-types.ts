export interface GrantToken {
  token: string
  path?: string
}

export interface HostResponse {
  status: 'ok' | 'error'
  error?: { code: string; message: string }
}

export interface DoubleLoveApi {
  hostHealth(): Promise<unknown>
  openSettings(): Promise<void>
  readonly dialogs: {
    pickDirectory(options: { title: string; kind: 'project-open' | 'model-root'; e2ePath?: string }): Promise<GrantToken | null>
    pickMediaFile(options?: { e2ePath?: string }): Promise<GrantToken | null>
    pickExportPath(options: { defaultName: string; kind: 'xml' | 'ass' | 'mp4'; e2ePath?: string }): Promise<GrantToken | null>
  }
  invoke(name: string, payload?: unknown): Promise<HostResponse>
  onEvent(channel: string, callback: (payload: unknown) => void): () => void
}

declare global {
  interface Window {
    readonly doubleLove: DoubleLoveApi
  }
}
