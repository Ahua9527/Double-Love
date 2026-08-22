import { execFileSync } from 'node:child_process'
import {
  accessSync,
  constants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { test, expect, _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import './api-types'

const require = createRequire(import.meta.url)
const electronExecutable = require('electron') as string
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const hostBinary = resolve(studioRoot, '../target/debug/double-love-desktop-host')
const mainEntry = resolve(studioRoot, 'out/main/index.js')

interface InvokeOperation<T> {
  status: 'success' | 'partial' | 'failed' | 'cancelled'
  revision: number | null
  data: T | null
  diagnostics: Array<{ code: string; blocks_export?: boolean }>
  outputs: Array<{ kind: string; path: string; sha256: string }>
}

interface InvokeEnvelope<T> {
  status: 'ok' | 'error'
  result?: { type: string; data?: InvokeOperation<T> }
  error?: { code: string }
}

interface MediaAssetSummary {
  id: string
  status: string
}

interface TranscriptView {
  words: Array<{ ordinal: number }>
  omits: Array<{ id: string }>
}

interface ExportOutcome {
  artifact_path: string | null
  sha256: string | null
  ir: { clips: unknown[] }
}

let electronApp: ElectronApplication
let page: Page
let temporaryRoot: string
let projectRoot: string
let mediaPath: string
let exportPath: string
let userDataPath: string

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

function stringValues(value: unknown): string[] {
  if (typeof value === 'string') return [value]
  if (Array.isArray(value)) return value.flatMap(stringValues)
  if (typeof value === 'object' && value !== null) {
    return Object.values(value).flatMap(stringValues)
  }
  return []
}

function generateMedia(path: string): void {
  execFileSync(process.env.DOUBLELOVE_FFMPEG || 'ffmpeg', [
    '-hide_banner',
    '-loglevel', 'error',
    '-y',
    '-f', 'lavfi',
    '-i', 'color=c=black:s=320x180:r=25:d=61',
    '-f', 'lavfi',
    '-i', 'sine=frequency=440:sample_rate=48000:duration=61',
    '-c:v', 'mpeg4',
    '-pix_fmt', 'yuv420p',
    '-c:a', 'aac',
    '-shortest',
    path,
  ])
}

function seedInstalledModels(userData: string): void {
  const modelRoot = join(userData, 'models')
  mkdirSync(modelRoot, { recursive: true })
  const installed = (modelId: string, revision: string) => ({
    model_id: modelId,
    revision,
    state: 'installed',
    bytes_downloaded: 0,
    bytes_total: 0,
    staging_id: null,
    last_error_code: null,
    last_error_message: null,
    updated_at: '2026-01-01T00:00:00Z',
  })
  writeFileSync(join(modelRoot, 'installations.json'), JSON.stringify({
    schema_version: 1,
    installations: {
      'qwen3-asr-0.6b': installed(
        'qwen3-asr-0.6b',
        '5eb144179a02acc5e5ba31e748d22b0cf3e303b0',
      ),
      'qwen3-forced-aligner-0.6b': installed(
        'qwen3-forced-aligner-0.6b',
        'c7cbfc2048c462b0d63a45797104fc9db3ad62b7',
      ),
    },
  }, null, 2))
}

async function invoke<T>(name: string, payload?: unknown): Promise<InvokeOperation<T>> {
  const response = await page.evaluate(
    async ({ command, commandPayload }) => window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  ) as InvokeEnvelope<T>
  expect(response.status).toBe('ok')
  expect(response.result?.type).toBe('invoke')
  return response.result?.data as InvokeOperation<T>
}

async function directoryGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickDirectory({
      title: 'Create synthetic Slice 5 project',
      kind: 'project-open',
      e2ePath,
    }),
    path,
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function mediaGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickMediaFile({ e2ePath }),
    path,
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function exportGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickExportPath({
      defaultName: 'slice5-rough-cut.xml',
      kind: 'xml',
      e2ePath,
    }),
    path,
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function eventStates(): Promise<{ progress: unknown[]; states: Array<{ task_id: string; state: string }> }> {
  return page.evaluate(() => {
    const captured = window as unknown as {
      slice5Progress: unknown[]
      slice5States: Array<{ task_id: string; state: string }>
    }
    return { progress: captured.slice5Progress, states: captured.slice5States }
  })
}

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-electron-slice5-e2e-'))
  projectRoot = join(temporaryRoot, 'project')
  mediaPath = join(temporaryRoot, 'slice5.mp4')
  exportPath = join(temporaryRoot, 'slice5-rough-cut.xml')
  userDataPath = join(temporaryRoot, 'user-data')
  generateMedia(mediaPath)
  seedInstalledModels(userDataPath)

  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      '--double-love-e2e',
      '--double-love-e2e-transcribe-mock',
      `--double-love-e2e-user-data=${userDataPath}`,
    ],
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  })
  page = await electronApp.firstWindow()
  await page.waitForLoadState('domcontentloaded')
  await page.evaluate(() => {
    const captured = window as unknown as {
      slice5Progress: unknown[]
      slice5States: Array<{ task_id: string; state: string }>
    }
    captured.slice5Progress = []
    captured.slice5States = []
    window.doubleLove.onEvent('dl://progress', (payload) => captured.slice5Progress.push(payload))
    window.doubleLove.onEvent('dl://task-state', (payload) => {
      captured.slice5States.push(payload as { task_id: string; state: string })
    })
  })
  await expect.poll(async () => {
    const response = await page.evaluate(() => window.doubleLove.hostHealth())
    return (response as { status?: string }).status
  }).toBe('ok')
})

test.afterAll(async () => {
  if (electronApp) await electronApp.close()
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true })
})

test('reveal commands open host paths without returning them to the renderer', async () => {
  const responses = await page.evaluate(() => Promise.all([
    window.doubleLove.invoke('model_reveal'),
    window.doubleLove.invoke('diagnostics_reveal_logs'),
  ])) as InvokeEnvelope<null>[]

  for (const response of responses) {
    expect(response).toMatchObject({
      status: 'ok',
      result: {
        type: 'invoke',
        data: { status: 'success', data: null },
      },
    })
    expect(response.result?.data?.data).toBeNull()
    expect(JSON.stringify(response)).not.toContain(userDataPath)
    expect(JSON.stringify(response)).not.toMatch(/"path":/u)
    expect(stringValues(response).filter((value) => isAbsolute(value))).toEqual([])
  }
})

test('transcribes with the test-only mock and applies a granted rough-cut export', async () => {
  test.setTimeout(60_000)
  expect(await invoke('project_create', {
    grantToken: await directoryGrant(projectRoot),
  })).toMatchObject({ status: 'success' })
  const imported = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(mediaPath),
  })
  expect(imported).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  const assetId = imported.data?.id as string

  const started = await invoke<{ task_id: string }>('transcribe_start', {
    assetId,
    model: 'qwen3-asr-0.6b',
    language: 'auto',
  })
  expect(started.status).toBe('success')
  const taskId = started.data?.task_id as string
  await expect.poll(async () => (await eventStates()).states.find((event) => event.task_id === taskId)?.state)
    .toBe('succeeded')
  const observed = await eventStates()
  expect(observed.progress.length).toBeGreaterThan(0)
  const capturedEvents = JSON.stringify(observed)
  expect(capturedEvents).not.toContain(projectRoot)
  expect(capturedEvents).not.toContain(mediaPath)

  const transcript = await invoke<TranscriptView>('transcript_get', { assetId })
  expect(transcript.status).toBe('success')
  expect(transcript.data?.words.length).toBeGreaterThan(100)
  const omitted = await invoke<{ id: string; handles_before_ms: number; handles_after_ms: number }>(
    'edit_omit',
    { assetId, startOrdinal: 1, endOrdinal: 2 },
  )
  expect(omitted).toMatchObject({
    status: 'success',
    data: { handles_before_ms: 120, handles_after_ms: 120 },
  })

  const preview = await invoke<ExportOutcome>('roughcut_preview', { assetId })
  expect(preview).toMatchObject({
    status: 'success',
    data: { artifact_path: null, sha256: null },
  })
  expect(existsSync(exportPath)).toBe(false)

  const applied = await invoke<ExportOutcome>('export_roughcut_apply', {
    assetId,
    grantToken: await exportGrant(exportPath),
  })
  expect(applied).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: {
      artifact_path: exportPath,
      sha256: expect.stringMatching(/^[a-f0-9]{64}$/u),
    },
    outputs: [{
      kind: 'premiere_xmeml',
      path: exportPath,
      sha256: expect.stringMatching(/^[a-f0-9]{64}$/u),
    }],
  })
  expect(readFileSync(exportPath, 'utf8')).toContain('<!DOCTYPE xmeml>')

  const restored = await invoke('edit_restore', {
    operationId: omitted.data?.id,
    startOrdinal: 1,
    endOrdinal: 2,
  })
  expect(restored.status).toBe('success')
  expect((await invoke<TranscriptView>('transcript_get', { assetId })).data?.omits).toEqual([])

  const cancelStarted = await invoke<{ task_id: string }>('transcribe_start', {
    assetId,
    model: 'qwen3-asr-0.6b',
    language: 'auto',
  })
  expect(cancelStarted.status).toBe('success')
  const cancelTaskId = cancelStarted.data?.task_id as string
  expect(await invoke('task_cancel', { taskId: cancelTaskId })).toMatchObject({ status: 'success' })
  await expect.poll(async () =>
    (await eventStates()).states.find((event) => event.task_id === cancelTaskId)?.state).toBe('cancelled')

  const wordCount = transcript.data?.words.length as number
  expect(await invoke('edit_omit', {
    assetId,
    startOrdinal: 0,
    endOrdinal: wordCount - 1,
  })).toMatchObject({ status: 'success' })
  const emptyPath = join(temporaryRoot, 'empty.xml')
  const empty = await invoke<ExportOutcome>('export_roughcut_apply', {
    assetId,
    grantToken: await exportGrant(emptyPath),
  })
  expect(empty.status).toBe('failed')
  expect(empty.diagnostics[0]).toMatchObject({ code: 'ROUGH_CUT_EMPTY', blocks_export: true })
  expect(existsSync(emptyPath)).toBe(false)

  const allCapturedEvents = JSON.stringify(await eventStates())
  expect(allCapturedEvents).not.toContain(projectRoot)
  expect(allCapturedEvents).not.toContain(mediaPath)
})
