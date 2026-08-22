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
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('renderer platform selector', () => {
  it('selects Electron when the preload bridge is present', async () => {
    setWindowProperty('doubleLove', {})
    const api = await loadDesktop()
    expect(api.platformKind).toBe('electron')
    expect(api.isDesktop).toBe(true)
  })

  it('selects browser preview without a desktop bridge', async () => {
    const api = await loadDesktop()
    expect(api.platformKind).toBe('preview')
    expect(api.isDesktop).toBe(false)
  })

  it.each(['file:', 'dl-app:'])('fails closed for a %s renderer without a desktop bridge', async (protocol) => {
    vi.stubGlobal('location', { protocol })
    await expect(loadDesktop()).rejects.toThrow('packaged renderer started without a desktop bridge')
  })
})
