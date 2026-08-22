import { appendFileSync, existsSync, mkdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

export type LogLevel = 'debug' | 'info' | 'warn' | 'error'
export type LogProcess = 'main' | 'host' | 'protocol' | 'updater'
export type LogStatus = 'start' | 'ok' | 'error' | 'stop' | 'cancelled' | 'crash'

export interface LocalLogEntry {
  level: LogLevel
  process: LogProcess
  requestId?: string
  method: string
  durationMs?: number
  status: LogStatus
  errorCode?: string
  hostVersion?: string
  engineVersion?: string
}

export interface PersistedLogEntry extends LocalLogEntry {
  ts: string
}

const DEFAULT_MAX_BYTES = 1024 * 1024
const DEFAULT_FILE_COUNT = 5
const SAFE_METHOD = /^[a-zA-Z][a-zA-Z0-9:._-]{0,127}$/u
const SAFE_REQUEST_ID = /^[a-zA-Z0-9-]{1,128}$/u
const SAFE_ERROR_CODE = /^[A-Z][A-Z0-9_]{0,127}$/u
const SAFE_VERSION = /^[0-9A-Za-z][0-9A-Za-z.+_-]{0,63}$/u

export class LocalLog {
  readonly logDirectory: string
  readonly crashMarkerPath: string
  private readonly activePath: string

  constructor(
    userDataPath: string,
    private readonly maxBytes = DEFAULT_MAX_BYTES,
    private readonly fileCount = DEFAULT_FILE_COUNT,
    private readonly now: () => Date = () => new Date(),
  ) {
    this.logDirectory = join(userDataPath, 'logs')
    this.activePath = join(this.logDirectory, 'main.jsonl')
    this.crashMarkerPath = join(this.logDirectory, 'host-crash.json')
    mkdirSync(this.logDirectory, { recursive: true })
  }

  write(input: LocalLogEntry & Record<string, unknown>): void {
    const record: PersistedLogEntry = {
      ts: this.now().toISOString(),
      level: input.level,
      process: input.process,
      method: SAFE_METHOD.test(input.method) ? input.method : 'invalid-method',
      status: input.status,
    }
    if (input.requestId && SAFE_REQUEST_ID.test(input.requestId)) record.requestId = input.requestId
    if (typeof input.durationMs === 'number' && Number.isFinite(input.durationMs) && input.durationMs >= 0) {
      record.durationMs = Math.round(input.durationMs * 1000) / 1000
    }
    if (input.errorCode && SAFE_ERROR_CODE.test(input.errorCode)) record.errorCode = input.errorCode
    if (input.hostVersion && SAFE_VERSION.test(input.hostVersion)) record.hostVersion = input.hostVersion
    if (input.engineVersion && SAFE_VERSION.test(input.engineVersion)) record.engineVersion = input.engineVersion

    const line = `${JSON.stringify(record)}\n`
    this.rotateIfNeeded(Buffer.byteLength(line))
    appendFileSync(this.activePath, line, { encoding: 'utf8', mode: 0o600 })
  }

  writeCrashMarker(exitCode: number | null, signal: NodeJS.Signals | null): void {
    const marker = {
      ts: this.now().toISOString(),
      exitCode,
      signal,
    }
    writeFileSync(this.crashMarkerPath, `${JSON.stringify(marker)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
    })
  }

  clearCrashMarker(): void {
    rmSync(this.crashMarkerPath, { force: true })
  }

  private rotateIfNeeded(incomingBytes: number): void {
    const currentBytes = existsSync(this.activePath) ? statSync(this.activePath).size : 0
    if (currentBytes === 0 || currentBytes + incomingBytes <= this.maxBytes) return

    const oldestArchive = this.fileCount - 1
    if (oldestArchive > 0) rmSync(`${this.activePath}.${oldestArchive}`, { force: true })
    for (let archive = oldestArchive - 1; archive >= 1; archive -= 1) {
      const source = `${this.activePath}.${archive}`
      if (existsSync(source)) renameSync(source, `${this.activePath}.${archive + 1}`)
    }
    if (this.fileCount > 1) renameSync(this.activePath, `${this.activePath}.1`)
    else rmSync(this.activePath, { force: true })
  }
}
