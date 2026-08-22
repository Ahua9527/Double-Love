import { listen as tauriListen } from '@tauri-apps/api/event'
import * as legacy from '../tauri'

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
