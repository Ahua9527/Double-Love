import { afterEach, describe, expect, it, vi } from 'vitest'
import { assetsList, importMedia, listen, pickDirectory } from './electron'

function installBridge(invoke: (name: string, payload?: unknown) => Promise<unknown>, onEvent = vi.fn()) {
  Object.defineProperty(window, 'doubleLove', {
    configurable: true,
    value: {
      hostHealth: vi.fn(),
      openSettings: vi.fn(),
      dialogs: {
        pickDirectory: vi.fn().mockResolvedValue({ token: 'directory-grant' }),
        pickMediaFile: vi.fn().mockResolvedValue(null),
        pickExportPath: vi.fn().mockResolvedValue(null),
      },
      invoke,
      onEvent,
    },
  })
  return window as unknown as { doubleLove: { dialogs: { pickDirectory: ReturnType<typeof vi.fn> } } }
}

afterEach(() => {
  Reflect.deleteProperty(window, 'doubleLove')
})

describe('Electron renderer adapter', () => {
  it('unwraps invoke HostResponse data and preserves command payloads', async () => {
    const operation = {
      status: 'success',
      revision: null,
      data: [],
      counts: { total: 0, processed: 0, skipped: 0, failed: 0, unmatched: 0 },
      diagnostics: [],
      outputs: [],
    }
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: 'request-1',
      status: 'ok',
      result: { type: 'invoke', data: operation },
    })
    installBridge(invoke)

    await expect(assetsList()).resolves.toBe(operation)
    expect(invoke).toHaveBeenCalledWith('assets_list', undefined)
  })

  it('maps HostResponse errors to failed OperationResult diagnostics', async () => {
    const invoke = vi.fn().mockResolvedValue({
      v: 1,
      id: 'request-2',
      status: 'error',
      error: { code: 'UNKNOWN_COMMAND', message: 'unknown command: import_media' },
    })
    installBridge(invoke)

    const result = await importMedia('opaque-token')
    expect(invoke).toHaveBeenCalledWith('import_media', { grantToken: 'opaque-token' })
    expect(result).toMatchObject({
      status: 'failed',
      data: null,
      counts: { total: 0, processed: 0, skipped: 0, failed: 1, unmatched: 0 },
      diagnostics: [{
        code: 'UNKNOWN_COMMAND',
        cause: 'unknown command: import_media',
        impact: '操作未产生可用结果',
        blocks_export: true,
      }],
    })
  })

  it('returns opaque dialog tokens and forwards the grant discriminator', async () => {
    const installed = installBridge(vi.fn())
    await expect(pickDirectory('选择已有项目', 'project-open')).resolves.toBe('directory-grant')
    await expect(pickDirectory('缺少授权类型')).rejects.toThrow('require a grant kind')
    expect(installed.doubleLove.dialogs.pickDirectory).toHaveBeenCalledWith({
      title: '选择已有项目',
      kind: 'project-open',
    })
  })

  it('maps event payloads and exposes the preload unsubscribe function', async () => {
    const unsubscribe = vi.fn()
    let hostCallback: ((payload: unknown) => void) | undefined
    const onEvent = vi.fn((_channel: string, callback: (payload: unknown) => void) => {
      hostCallback = callback
      return unsubscribe
    })
    installBridge(vi.fn(), onEvent)
    const callback = vi.fn()

    const remove = await listen<{ completed: number }>('dl://progress', callback)
    hostCallback?.({ completed: 3 })
    expect(callback).toHaveBeenCalledWith({ payload: { completed: 3 } })
    remove()
    expect(unsubscribe).toHaveBeenCalledOnce()
  })
})
