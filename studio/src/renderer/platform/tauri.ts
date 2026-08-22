import { getName, getVersion } from '@tauri-apps/api/app'
import { listen as tauriListen } from '@tauri-apps/api/event'
import * as legacy from '../tauri'
import type { UpdateStatus } from './normalize'

export * from '../tauri'

export const isDesktop = true
export const platformKind = 'tauri' as const

export function listen<T>(channel: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  return tauriListen<T>(channel, callback)
}

export function pickDirectory(title: string, kind?: 'project-open' | 'model-root') {
  void kind
  return legacy.pickDirectory(title)
}

export async function getAppInfo() {
  const [name, version] = await Promise.all([getName(), getVersion()])
  return { name, version }
}

function unsupportedUpdate(): Promise<UpdateStatus> {
  return Promise.resolve({ stage: 'error', error: '当前版本不支持应用内更新。' })
}

export const updateCheck = unsupportedUpdate
export const updateDownload = unsupportedUpdate
export const updateInstall = unsupportedUpdate
