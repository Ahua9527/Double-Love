import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'

const swState = vi.hoisted(() => ({
  offlineReady: false,
  needRefresh: false,
  setOfflineReady: vi.fn(),
  setNeedRefresh: vi.fn(),
  updateServiceWorker: vi.fn(),
}))

vi.mock('virtual:pwa-register/react', () => ({
  useRegisterSW: () => ({
    offlineReady: [swState.offlineReady, swState.setOfflineReady],
    needRefresh: [swState.needRefresh, swState.setNeedRefresh],
    updateServiceWorker: swState.updateServiceWorker,
  }),
}))

import PWAUpdatePrompt from './PWAUpdatePrompt'

beforeEach(() => {
  swState.offlineReady = false
  swState.needRefresh = false
  vi.clearAllMocks()
  swState.setNeedRefresh.mockImplementation((value: boolean) => {
    swState.needRefresh = value
  })
})

afterEach(() => cleanup())

describe('PWAUpdatePrompt', () => {
  it('新版本可用时由用户确认更新，不自动替换当前页面', () => {
    swState.needRefresh = true
    render(<PWAUpdatePrompt />)

    expect(screen.getByRole('alert').textContent).toContain('新版本可用')
    fireEvent.click(screen.getByRole('button', { name: '确认更新' }))
    expect(swState.updateServiceWorker).toHaveBeenCalledWith(true)
  })

  it('离线资源准备完成后不显示悬浮提示', () => {
    swState.offlineReady = true
    render(<PWAUpdatePrompt />)

    expect(screen.queryByRole('alert')).toBeNull()
    expect(swState.setOfflineReady).not.toHaveBeenCalled()
  })

  it('关闭更新提示后消失', () => {
    swState.needRefresh = true
    const { rerender } = render(<PWAUpdatePrompt />)

    fireEvent.click(screen.getByRole('button', { name: '关闭提示' }))
    expect(swState.setNeedRefresh).toHaveBeenCalledWith(false)
    rerender(<PWAUpdatePrompt />)
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
