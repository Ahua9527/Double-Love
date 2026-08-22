// @vitest-environment node

import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { LocalLog } from './local-log'

describe('LocalLog', () => {
  let directory: string

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'double-love-local-log-'))
  })

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true })
  })

  it('rotates JSONL while retaining five files total', () => {
    const log = new LocalLog(directory, 180, 5, () => new Date('2026-08-22T00:00:00.000Z'))
    for (let index = 0; index < 20; index += 1) {
      log.write({
        level: 'info',
        process: 'main',
        method: 'ipc.invoke',
        status: 'ok',
        requestId: `request-${index}`,
      })
    }

    const files = readdirSync(log.logDirectory).filter((name) => name.startsWith('main.jsonl'))
    expect(files).toHaveLength(5)
    expect(files.sort()).toEqual([
      'main.jsonl',
      'main.jsonl.1',
      'main.jsonl.2',
      'main.jsonl.3',
      'main.jsonl.4',
    ])
  })

  it('persists only the approved field allowlist and redacts unsafe identifiers', () => {
    const log = new LocalLog(directory)
    log.write({
      level: 'error',
      process: 'main',
      requestId: 'safe-request-id',
      method: '/Users/example/private.mov',
      durationMs: 12.3456,
      status: 'error',
      errorCode: 'IMPORT_FAILED',
      hostVersion: '0.1.0',
      engineVersion: '0.1.0',
      path: '/Users/example/private.mov',
      token: 'secret-token',
      payload: { transcript: 'private media text' },
      speaker: 'private speaker',
    })

    const line = readFileSync(join(log.logDirectory, 'main.jsonl'), 'utf8').trim()
    const record = JSON.parse(line) as Record<string, unknown>
    expect(Object.keys(record).sort()).toEqual([
      'durationMs',
      'engineVersion',
      'errorCode',
      'hostVersion',
      'level',
      'method',
      'process',
      'requestId',
      'status',
      'ts',
    ])
    expect(record.method).toBe('invalid-method')
    expect(line).not.toContain('/Users')
    expect(line).not.toContain('secret-token')
    expect(line).not.toContain('private media text')
    expect(line).not.toContain('private speaker')
  })

  it('writes and clears the host crash marker', () => {
    const log = new LocalLog(directory, 1024, 5, () => new Date('2026-08-22T00:00:00.000Z'))
    log.writeCrashMarker(9, 'SIGKILL')
    expect(JSON.parse(readFileSync(log.crashMarkerPath, 'utf8'))).toEqual({
      ts: '2026-08-22T00:00:00.000Z',
      exitCode: 9,
      signal: 'SIGKILL',
    })
    log.clearCrashMarker()
    expect(() => readFileSync(log.crashMarkerPath)).toThrow()
  })
})
