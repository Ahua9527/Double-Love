import { contextBridge, ipcRenderer } from 'electron'

const ALLOWED_HOST_EVENTS = new Set([
  'dl://progress',
  'dl://task-state',
  'dl://model-progress',
  'dl://model-state',
  'dl://preferences-changed',
  'dl://doctor-result',
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

const doubleLove = Object.freeze({
  hostHealth: (): Promise<unknown> => ipcRenderer.invoke('dl:host-health'),
  openSettings: (): Promise<void> => ipcRenderer.invoke('app:open-settings'),
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
