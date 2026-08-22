import { contextBridge, ipcRenderer } from 'electron'

const doubleLove = Object.freeze({
  hostHealth: (): Promise<unknown> => ipcRenderer.invoke('dl:host-health'),
  openSettings: (): Promise<void> => ipcRenderer.invoke('app:open-settings'),
})

contextBridge.exposeInMainWorld('doubleLove', doubleLove)
