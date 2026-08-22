import { chmodSync, writeFileSync } from 'node:fs'
import type { AppUpdater, Logger, ProgressInfo, UpdateInfo } from 'electron-updater'
import type { LocalLog } from './local-log'

export const UPDATE_STATUS_CHANNEL = 'dl://update-status'

export type UpdateStage =
  | 'idle'
  | 'checking-update'
  | 'update-available'
  | 'update-not-available'
  | 'download-progress'
  | 'update-downloaded'
  | 'error'

export interface UpdateStatus {
  stage: UpdateStage
  version?: string
  percent?: number
  error?: string
}

export interface UpdaterControllerOptions {
  updater: AppUpdater
  log: LocalLog
  broadcast: (channel: typeof UPDATE_STATUS_CHANNEL, payload: UpdateStatus) => void
  isPackaged: boolean
  e2eEnabled: boolean
  feedUrl?: string
  feedConfigPath?: string
  setInstalling: (installing: boolean) => void
}

const UPDATE_ERROR_MESSAGE = '暂时无法完成更新操作，请稍后重试。'
const SAFE_VERSION = /^[0-9A-Za-z][0-9A-Za-z.+_-]{0,63}$/u
const SAFE_ERROR_CODE = /^[A-Z][A-Z0-9_]{0,127}$/u

function safeVersion(value: unknown): string | undefined {
  return typeof value === 'string' && SAFE_VERSION.test(value) ? value : undefined
}

function safePercent(value: unknown): number | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value)) return undefined
  return Math.round(Math.min(100, Math.max(0, value)) * 10) / 10
}

function updaterErrorCode(error: unknown): string {
  if (typeof error !== 'object' || error === null) return 'UPDATE_ERROR'
  const code = Reflect.get(error, 'code')
  return typeof code === 'string' && SAFE_ERROR_CODE.test(code) ? code : 'UPDATE_ERROR'
}

export function updateFeedOverride(
  feedUrl: string | undefined,
  isPackaged: boolean,
  e2eEnabled: boolean,
): string | null {
  if (!feedUrl || (isPackaged && !e2eEnabled)) return null
  try {
    const parsed = new URL(feedUrl)
    if (
      !['http:', 'https:'].includes(parsed.protocol)
      || parsed.username
      || parsed.password
      || parsed.search
      || parsed.hash
    ) return null
    return parsed.toString()
  } catch {
    return null
  }
}

function eventPayload(status: UpdateStatus): UpdateStatus {
  const payload: UpdateStatus = { stage: status.stage }
  if (status.version) payload.version = status.version
  if (status.percent !== undefined) payload.percent = status.percent
  return payload
}

export class UpdaterController {
  private status: UpdateStatus = { stage: 'idle' }
  private availableVersion: string | null = null
  private downloadedVersion: string | null = null
  private startupCheckStarted = false
  private silentStartupCheck = false

  constructor(private readonly options: UpdaterControllerOptions) {
    const { updater } = options
    updater.autoDownload = false
    updater.autoInstallOnAppQuit = false
    updater.allowPrerelease = false
    updater.logger = this.localLogger()

    const feedOverride = updateFeedOverride(options.feedUrl, options.isPackaged, options.e2eEnabled)
    if (feedOverride) {
      if (options.feedConfigPath) {
        writeFileSync(
          options.feedConfigPath,
          `provider: generic\nurl: ${JSON.stringify(feedOverride)}\nupdaterCacheDirName: double-love-studio-updater-e2e\n`,
          { encoding: 'utf8', mode: 0o600 },
        )
        chmodSync(options.feedConfigPath, 0o600)
        updater.updateConfigPath = options.feedConfigPath
      }
      updater.setFeedURL({ provider: 'generic', url: feedOverride })
      if (!options.isPackaged) updater.forceDevUpdateConfig = true
    }

    updater.on('checking-for-update', () => {
      this.publish({ stage: 'checking-update' }, !this.silentStartupCheck)
    })
    updater.on('update-available', (info: UpdateInfo) => {
      const version = safeVersion(info.version)
      this.availableVersion = version ?? null
      this.downloadedVersion = null
      this.publish({ stage: 'update-available', ...(version ? { version } : {}) })
    })
    updater.on('update-not-available', () => {
      this.availableVersion = null
      this.downloadedVersion = null
      this.publish({ stage: 'update-not-available' })
    })
    updater.on('download-progress', (progress: ProgressInfo) => {
      const percent = safePercent(progress.percent)
      this.publish({
        stage: 'download-progress',
        ...(this.availableVersion ? { version: this.availableVersion } : {}),
        ...(percent !== undefined ? { percent } : {}),
      })
    })
    updater.on('update-downloaded', (info: UpdateInfo) => {
      const version = safeVersion(info.version) ?? this.availableVersion ?? undefined
      this.downloadedVersion = version ?? null
      this.publish({ stage: 'update-downloaded', ...(version ? { version } : {}) })
    })
    updater.on('error', (error) => {
      this.options.log.write({
        level: 'error',
        process: 'updater',
        method: 'lifecycle.error',
        status: 'error',
        errorCode: updaterErrorCode(error),
      })
      this.publish({ stage: 'error' }, !this.silentStartupCheck)
    })
  }

  async checkOnStartup(): Promise<void> {
    if (!this.options.isPackaged || this.startupCheckStarted) return
    this.startupCheckStarted = true
    this.silentStartupCheck = true
    try {
      await this.options.updater.checkForUpdates()
    } catch {
      this.options.log.write({
        level: 'error',
        process: 'updater',
        method: 'startup.check',
        status: 'error',
        errorCode: 'UPDATE_CHECK_FAILED',
      })
    } finally {
      this.silentStartupCheck = false
    }
  }

  async checkManually(): Promise<UpdateStatus> {
    this.publish({ stage: 'checking-update' })
    try {
      const result = await this.options.updater.checkForUpdates()
      if (!result && this.status.stage === 'checking-update') {
        return this.manualError('当前运行方式不支持检查更新。')
      }
      return this.status
    } catch {
      return this.manualError(UPDATE_ERROR_MESSAGE)
    }
  }

  async download(): Promise<UpdateStatus> {
    if (!this.availableVersion || this.downloadedVersion) {
      return this.manualError('请先检查并确认有可用更新。')
    }
    this.publish({
      stage: 'download-progress',
      version: this.availableVersion,
      percent: 0,
    })
    try {
      await this.options.updater.downloadUpdate()
      return this.status
    } catch {
      return this.manualError(UPDATE_ERROR_MESSAGE)
    }
  }

  install(): UpdateStatus {
    if (!this.downloadedVersion) {
      return this.manualError('更新尚未下载完成。')
    }

    this.options.setInstalling(true)
    try {
      this.options.updater.quitAndInstall(false, false)
      return this.status
    } catch {
      this.options.setInstalling(false)
      return this.manualError(UPDATE_ERROR_MESSAGE)
    }
  }

  private manualError(error: string): UpdateStatus {
    this.status = { stage: 'error', error }
    this.options.broadcast(UPDATE_STATUS_CHANNEL, eventPayload(this.status))
    return this.status
  }

  private publish(status: UpdateStatus, shouldBroadcast = true): void {
    this.status = status
    if (shouldBroadcast) this.options.broadcast(UPDATE_STATUS_CHANNEL, eventPayload(status))
  }

  private localLogger(): Logger {
    const write = (level: 'info' | 'warn' | 'error') => () => {
      this.options.log.write({
        level,
        process: 'updater',
        method: 'electron-updater',
        status: level === 'error' ? 'error' : 'ok',
        ...(level === 'error' ? { errorCode: 'UPDATE_ERROR' } : {}),
      })
    }
    return {
      info: write('info'),
      warn: write('warn'),
      error: write('error'),
      debug: write('info'),
    }
  }
}
