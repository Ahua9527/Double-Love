import { afterEach, describe, expect, it, vi } from 'vitest'

function setWindowProperty(name: string, value: unknown): void {
  Object.defineProperty(window, name, { configurable: true, value })
}

async function loadDesktop() {
  vi.resetModules()
  return import('./desktop')
}

afterEach(() => {
  Reflect.deleteProperty(window, 'doubleLove')
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__')
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('renderer platform selector', () => {
  it('selects Electron when the preload bridge is present', async () => {
    setWindowProperty('doubleLove', {})
    setWindowProperty('__TAURI_INTERNALS__', {})
    const api = await loadDesktop()
    expect(api.platformKind).toBe('electron')
    expect(api.isDesktop).toBe(true)
  })

  it('selects Tauri when only the Tauri bridge is present', async () => {
    setWindowProperty('__TAURI_INTERNALS__', {})
    const api = await loadDesktop()
    expect(api.platformKind).toBe('tauri')
    expect(api.isDesktop).toBe(true)
  })

  it('selects browser preview without a desktop bridge', async () => {
    const api = await loadDesktop()
    expect(api.platformKind).toBe('preview')
    expect(api.isDesktop).toBe(false)
  })

  it('fails closed for a file renderer without a desktop bridge', async () => {
    vi.stubGlobal('location', { protocol: 'file:' })
    await expect(loadDesktop()).rejects.toThrow('packaged renderer started without a desktop bridge')
  })
})
