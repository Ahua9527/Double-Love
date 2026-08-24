// @vitest-environment node

import { describe, expect, it, vi } from 'vitest'
import { createQuitFlowState, handleBeforeQuit } from './quit-flow'

describe('before-quit flow', () => {
  it('does not prevent quitAndInstall and stops the host without waiting', () => {
    const state = createQuitFlowState()
    state.installingUpdate = true
    const preventDefault = vi.fn()
    const stop = vi.fn(() => Promise.resolve())
    const stopImmediately = vi.fn()
    const quit = vi.fn()

    handleBeforeQuit({ preventDefault }, state, { stop, stopImmediately }, quit)

    expect(preventDefault).not.toHaveBeenCalled()
    expect(stopImmediately).toHaveBeenCalledOnce()
    expect(stop).not.toHaveBeenCalled()
    expect(quit).not.toHaveBeenCalled()
    expect(state.allowQuit).toBe(true)
  })

  it('keeps the normal graceful shutdown gate for an ordinary quit', async () => {
    const state = createQuitFlowState()
    const preventDefault = vi.fn()
    const stop = vi.fn(() => Promise.resolve())
    const stopImmediately = vi.fn()
    const quit = vi.fn()

    handleBeforeQuit({ preventDefault }, state, { stop, stopImmediately }, quit)
    await vi.waitFor(() => expect(quit).toHaveBeenCalledOnce())

    expect(preventDefault).toHaveBeenCalledOnce()
    expect(stop).toHaveBeenCalledOnce()
    expect(stopImmediately).not.toHaveBeenCalled()
    expect(state.allowQuit).toBe(true)
  })
})
