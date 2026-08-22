import { contextBridge, ipcRenderer } from 'electron'

const ALLOWED_HOST_EVENTS = new Set([
  'dl://progress',
  'dl://task-state',
  'dl://model-progress',
  'dl://model-state',
  'dl://preferences-changed',
  'dl://doctor-result',
  'dl://update-status',
])

export type DirectoryGrantKind = 'project-open' | 'model-root'
export type ExportKind = 'xml' | 'ass' | 'mp4'

export interface GrantToken {
  token: string
}

interface E2eOverride {
  e2ePath?: string
}

const dialogs = Object.freeze({
  pickDirectory: (options: { title: string; kind: DirectoryGrantKind } & E2eOverride): Promise<GrantToken | null> =>
    ipcRenderer.invoke('dl:dialog-pick-directory', options),
  pickMediaFile: (options?: E2eOverride): Promise<GrantToken | null> =>
    ipcRenderer.invoke('dl:dialog-pick-media-file', options),
  pickExportPath: (options: { defaultName: string; kind: ExportKind } & E2eOverride): Promise<GrantToken | null> =>
    ipcRenderer.invoke('dl:dialog-pick-export-path', options),
})

const updates = Object.freeze({
  check: (): Promise<unknown> => ipcRenderer.invoke('update:check'),
  download: (): Promise<unknown> => ipcRenderer.invoke('update:download'),
  install: (): Promise<unknown> => ipcRenderer.invoke('update:install'),
})

const doubleLove = Object.freeze({
  hostHealth: (): Promise<unknown> => ipcRenderer.invoke('dl:host-health'),
  openSettings: (): Promise<void> => ipcRenderer.invoke('app:open-settings'),
  getAppInfo: (): Promise<unknown> => ipcRenderer.invoke('app:get-info'),
  updates,
  dialogs,
  invoke: (name: string, payload?: unknown): Promise<unknown> => ipcRenderer.invoke('dl:invoke', name, payload),
  onEvent: (channel: string, callback: (payload: unknown) => void): (() => void) => {
    if (!ALLOWED_HOST_EVENTS.has(channel)) {
      throw new TypeError('Unsupported host event channel')
    }
    if (typeof callback !== 'function') throw new TypeError('Host event callback must be a function')

    const listener = (_event: Electron.IpcRendererEvent, eventName: string, payload: unknown): void => {
      if (eventName === channel) callback(payload)
    }
    ipcRenderer.on('dl:host-event', listener)
    return () => ipcRenderer.removeListener('dl:host-event', listener)
  },
})

contextBridge.exposeInMainWorld('doubleLove', doubleLove)
