// @vitest-environment node

import { EventEmitter } from 'node:events'
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { AppUpdater } from 'electron-updater'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { LocalLog } from './local-log'
import { UpdaterController, updateFeedOverride, type UpdateStatus } from './updater'

class FakeUpdater extends EventEmitter {
  autoDownload = true
  autoInstallOnAppQuit = true
  allowPrerelease = true
  forceDevUpdateConfig = false
  updateConfigPath: string | null = null
  logger: unknown = console
  readonly setFeedURL = vi.fn()
  readonly quitAndInstall = vi.fn()
  readonly checkForUpdates = vi.fn(async () => {
    this.emit('checking-for-update')
    const info = { version: '0.2.1-feed' }
    this.emit('update-available', info)
    return { isUpdateAvailable: true, updateInfo: info, versionInfo: info }
  })
  readonly downloadUpdate = vi.fn(async () => {
    this.emit('download-progress', {
      percent: 52.345,
      bytesPerSecond: 1,
      transferred: 1,
      total: 2,
    })
    this.emit('update-downloaded', {
      version: '0.2.1-feed',
      downloadedFile: '/private/update.zip',
      files: [{ url: 'http://127.0.0.1/private.zip' }],
    })
    return ['/private/update.zip']
  })
}

const directories: string[] = []
afterEach(() => {
  for (const directory of directories.splice(0)) {
    rmSync(directory, { recursive: true, force: true })
  }
})

function controller(fake: FakeUpdater, broadcasts: UpdateStatus[], setInstalling = vi.fn()) {
  const directory = mkdtempSync(join(tmpdir(), 'double-love-updater-'))
  directories.push(directory)
  return new UpdaterController({
    updater: fake as unknown as AppUpdater,
    log: new LocalLog(directory),
    broadcast: (_channel, payload) => broadcasts.push(payload),
    isPackaged: true,
    e2eEnabled: true,
    feedUrl: 'http://127.0.0.1:43123/feed/',
    feedConfigPath: join(directory, 'e2e-app-update.yml'),
    setInstalling,
  })
}

describe('UpdaterController', () => {
  it('configures explicit-download stable updates and gates install on download completion', async () => {
    const fake = new FakeUpdater()
    const broadcasts: UpdateStatus[] = []
    const setInstalling = vi.fn()
    const updates = controller(fake, broadcasts, setInstalling)

    expect(fake.autoDownload).toBe(false)
    expect(fake.autoInstallOnAppQuit).toBe(false)
    expect(fake.allowPrerelease).toBe(false)
    expect(fake.setFeedURL).toHaveBeenCalledWith({
      provider: 'generic',
      url: 'http://127.0.0.1:43123/feed/',
    })
    expect(fake.updateConfigPath).not.toBeNull()
    expect(readFileSync(fake.updateConfigPath!, 'utf8')).toContain('updaterCacheDirName: double-love-studio-updater-e2e')
    expect(statSync(fake.updateConfigPath!).mode & 0o777).toBe(0o600)

    expect(updates.install()).toMatchObject({ stage: 'error' })
    expect(fake.quitAndInstall).not.toHaveBeenCalled()

    await expect(updates.checkManually()).resolves.toEqual({
      stage: 'update-available',
      version: '0.2.1-feed',
    })
    expect(fake.downloadUpdate).not.toHaveBeenCalled()

    await expect(updates.download()).resolves.toEqual({
      stage: 'update-downloaded',
      version: '0.2.1-feed',
    })
    expect(updates.install()).toEqual({
      stage: 'update-downloaded',
      version: '0.2.1-feed',
    })
    expect(setInstalling).toHaveBeenLastCalledWith(true)
    expect(fake.quitAndInstall).toHaveBeenCalledWith(false, false)

    expect(broadcasts).toContainEqual({
      stage: 'download-progress',
      version: '0.2.1-feed',
      percent: 52.3,
    })
    expect(broadcasts).toContainEqual({
      stage: 'update-downloaded',
      version: '0.2.1-feed',
    })
    expect(JSON.stringify(broadcasts)).not.toContain('127.0.0.1')
    expect(JSON.stringify(broadcasts)).not.toContain('/private')
  })

  it('returns a readable error for a failed manual check while keeping event payloads sanitized', async () => {
    const fake = new FakeUpdater()
    fake.checkForUpdates.mockImplementationOnce(async () => {
      const error = Object.assign(new Error('/private/feed/token'), { code: 'ERR_UPDATER_TEST' })
      fake.emit('error', error)
      throw error
    })
    const broadcasts: UpdateStatus[] = []
    const updates = controller(fake, broadcasts)

    await expect(updates.checkManually()).resolves.toEqual({
      stage: 'error',
      error: '暂时无法完成更新操作，请稍后重试。',
    })
    expect(broadcasts.at(-1)).toEqual({ stage: 'error' })
    expect(JSON.stringify(broadcasts)).not.toContain('/private')
  })

  it('keeps a failed packaged startup check out of renderer events', async () => {
    const fake = new FakeUpdater()
    fake.checkForUpdates.mockImplementationOnce(async () => {
      fake.emit('checking-for-update')
      const error = new Error('http://127.0.0.1/private/token')
      fake.emit('error', error)
      throw error
    })
    const broadcasts: UpdateStatus[] = []
    const updates = controller(fake, broadcasts)

    await expect(updates.checkOnStartup()).resolves.toBeUndefined()
    await expect(updates.checkOnStartup()).resolves.toBeUndefined()
    expect(fake.checkForUpdates).toHaveBeenCalledOnce()
    expect(broadcasts).toEqual([])
  })
})

describe('update feed override gate', () => {
  it('ignores packaged environment overrides outside the explicit E2E switch', () => {
    expect(updateFeedOverride('http://127.0.0.1:9000/', true, false)).toBeNull()
    expect(updateFeedOverride('http://user:token@127.0.0.1:9000/', true, true)).toBeNull()
    expect(updateFeedOverride('file:///private/feed', false, false)).toBeNull()
    expect(updateFeedOverride('http://127.0.0.1:9000/?token=secret', true, true)).toBeNull()
    expect(updateFeedOverride('http://127.0.0.1:9000/', false, false)).toBe('http://127.0.0.1:9000/')
  })
})
