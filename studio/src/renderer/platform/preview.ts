export * from '../tauri'

export const isDesktop = false
export const platformKind = 'preview' as const

export function getAppInfo() {
  return Promise.resolve({ name: 'Double Love Studio', version: '0.2.0' })
}

export function updateCheck() {
  return Promise.resolve({ stage: 'error' as const, error: '浏览器预览不支持检查更新。' })
}

export const updateDownload = updateCheck
export const updateInstall = updateCheck

export function listen<T>(channel: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  void channel
  void callback
  return Promise.reject(new Error('Desktop event bridge is unavailable in browser preview'))
}
