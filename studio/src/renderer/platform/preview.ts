export * from '../tauri'

export const isDesktop = false
export const platformKind = 'preview' as const

export function listen<T>(channel: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  void channel
  void callback
  return Promise.reject(new Error('Desktop event bridge is unavailable in browser preview'))
}
