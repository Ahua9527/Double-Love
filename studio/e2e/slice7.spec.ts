import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  accessSync,
  chmodSync,
  constants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { test, expect, _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import './api-types'

const require = createRequire(import.meta.url)
const electronExecutable = require('electron') as string
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = resolve(studioRoot, '..')
const hostBinary = resolve(repositoryRoot, 'target/debug/double-love-desktop-host')
const cliBinary = resolve(repositoryRoot, 'target/debug/double-love')
const mainEntry = resolve(studioRoot, 'out/main/index.js')

interface Diagnostic {
  code: string
  cause: string
  blocks_export: boolean
}

interface OutputArtifact {
  kind: string
  path: string
  sha256: string | null
}

interface InvokeOperation<T> {
  status: 'success' | 'partial' | 'failed' | 'cancelled'
  revision: number | null
  data: T | null
  diagnostics: Diagnostic[]
  outputs: OutputArtifact[]
}

interface InvokeEnvelope<T> {
  status: 'ok' | 'error'
  result?: { type: string; data?: InvokeOperation<T> }
  error?: { code: string; message: string }
}

interface MediaAssetSummary {
  id: string
  status: string
}

interface MainTrackClip {
  id: string
  source_asset_id: string
}

interface TranscriptView {
  words: Array<{ ordinal: number; display_text: string }>
}

interface ProjectExportPreview {
  timeline: {
    schema_version: number
    name: string
    rate: string
    sources: Array<{ asset_id: string; original_path: string; rate: string }>
    clips: Array<{ source_asset_id: string }>
    output_duration_frames: number
  }
  subtitle_cues: Array<{ text: string; start_frame: number; end_frame: number }>
  compatibility: Array<{ target: string; preserved: string[]; limitations: string[] }>
}

interface CapturedTaskState {
  task_id: string
  state: string
}

let electronApp: ElectronApplication
let page: Page
let temporaryRoot: string
let projectRoot: string
let firstMediaPath: string
let secondMediaPath: string
let userDataPath: string
let delayedFfmpegPath: string

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(cliBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

function generateMedia(
  path: string,
  fps: string,
  color: string,
  frequency: number,
): void {
  execFileSync(process.env.DOUBLELOVE_FFMPEG || 'ffmpeg', [
    '-hide_banner',
    '-loglevel', 'error',
    '-y',
    '-f', 'lavfi',
    '-i', `color=c=${color}:s=320x180:r=${fps}:d=2`,
    '-f', 'lavfi',
    '-i', `sine=frequency=${frequency}:sample_rate=48000:duration=2`,
    '-c:v', 'mpeg4',
    '-pix_fmt', 'yuv420p',
    '-c:a', 'aac',
    '-shortest',
    path,
  ])
}

function writeDelayedRenderFfmpeg(path: string): void {
  const configuredFfmpeg = process.env.DOUBLELOVE_FFMPEG
  const actualFfmpeg = configuredFfmpeg
    ? realpathSync(configuredFfmpeg)
    : execFileSync('/usr/bin/which', ['ffmpeg'], { encoding: 'utf8' }).trim()
  const shellPath = `'${actualFfmpeg.replaceAll("'", "'\\''")}'`
  writeFileSync(path, `#!/bin/sh\ndelay=0\nfor argument in "$@"; do\n  if [ "$argument" = "-filter_complex" ]; then delay=1; fi\ndone\nif [ "$delay" -eq 1 ]; then sleep 6; fi\nexec ${shellPath} "$@"\n`)
  chmodSync(path, 0o755)
}

function seedInstalledModels(userData: string): void {
  const modelRoot = join(userData, 'models')
  mkdirSync(modelRoot, { recursive: true })
  const installed = (modelId: string, revision: string) => {
    mkdirSync(join(modelRoot, modelId, revision), { recursive: true })
    return {
      model_id: modelId,
      revision,
      state: 'installed',
      bytes_downloaded: 0,
      bytes_total: 0,
      staging_id: null,
      last_error_code: null,
      last_error_message: null,
      updated_at: '2026-01-01T00:00:00Z',
    }
  }
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

async function rawInvoke<T>(name: string, payload?: unknown): Promise<InvokeEnvelope<T>> {
  return page.evaluate(
    async ({ command, commandPayload }) => window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  ) as Promise<InvokeEnvelope<T>>
}

async function invoke<T>(name: string, payload?: unknown): Promise<InvokeOperation<T>> {
  const response = await rawInvoke<T>(name, payload)
  expect(response.status).toBe('ok')
  expect(response.result?.type).toBe('invoke')
  return response.result?.data as InvokeOperation<T>
}

async function directoryGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickDirectory({
      title: 'Create synthetic Slice 7 project',
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

async function exportGrant(
  path: string,
  kind: 'xml' | 'ass' | 'mp4',
): Promise<string> {
  const grant = await page.evaluate(
    async ({ e2ePath, exportKind }) => window.doubleLove.dialogs.pickExportPath({
      defaultName: `slice7-rough-cut.${exportKind}`,
      kind: exportKind,
      e2ePath,
    }),
    { e2ePath: path, exportKind: kind },
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function taskStates(): Promise<CapturedTaskState[]> {
  return page.evaluate(() => {
    const captured = window as unknown as { slice7TaskStates: CapturedTaskState[] }
    return captured.slice7TaskStates
  })
}

async function transcribeAndWait(assetId: string): Promise<void> {
  const started = await invoke<{ task_id: string }>('transcribe_start', {
    assetId,
    model: 'qwen3-asr-0.6b',
    language: 'auto',
  })
  expect(started.status).toBe('success')
  const taskId = started.data?.task_id as string
  await expect.poll(async () =>
    (await taskStates()).find((state) => state.task_id === taskId)?.state).toBe('succeeded')
}

function sha256(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function expectArtifact(
  operation: InvokeOperation<ProjectExportPreview>,
  kind: string,
  path: string,
): void {
  expect(operation).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    outputs: [{
      kind,
      path,
      sha256: expect.stringMatching(/^[a-f0-9]{64}$/u),
    }],
  })
  expect(operation.outputs[0].sha256).toBe(sha256(path))
}

function cliExportXmeml(path: string): InvokeOperation<ProjectExportPreview> {
  return JSON.parse(execFileSync(cliBinary, [
    '--json',
    '--project', projectRoot,
    'export-project',
    'xml',
    '--apply',
    '--out', path,
  ], { encoding: 'utf8' })) as InvokeOperation<ProjectExportPreview>
}

function exportLedger(database: string): Array<{ kind: string; path: string; sha256: string }> {
  const rows = execFileSync('sqlite3', [
    database,
    "SELECT kind || char(9) || path || char(9) || sha256 FROM export_artifact ORDER BY revision;",
  ], { encoding: 'utf8' }).trim()
  if (!rows) return []
  return rows.split('\n').map((row) => {
    const [kind, path, hash] = row.split('\t')
    return { kind, path, sha256: hash }
  })
}

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-electron-slice7-e2e-'))
  projectRoot = join(temporaryRoot, 'project')
  firstMediaPath = join(temporaryRoot, 'slice 7 first-25.mp4')
  secondMediaPath = join(temporaryRoot, 'slice 7 second-30000-1001.mp4')
  userDataPath = join(temporaryRoot, 'user-data')
  delayedFfmpegPath = join(temporaryRoot, 'ffmpeg-delayed-render')
  generateMedia(firstMediaPath, '25', 'steelblue', 440)
  generateMedia(secondMediaPath, '30000/1001', 'seagreen', 660)
  writeDelayedRenderFfmpeg(delayedFfmpegPath)
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
      DOUBLELOVE_FFMPEG: delayedFfmpegPath,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  })
  page = await electronApp.firstWindow()
  await page.waitForLoadState('domcontentloaded')
  await page.evaluate(() => {
    const captured = window as unknown as { slice7TaskStates: CapturedTaskState[] }
    captured.slice7TaskStates = []
    window.doubleLove.onEvent('dl://task-state', (payload) => {
      captured.slice7TaskStates.push(payload as CapturedTaskState)
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

test('delivers preview, XMEML, ASS, and a real MP4 through one-time grants', async () => {
  test.setTimeout(120_000)

  expect(await invoke('project_create', {
    grantToken: await directoryGrant(projectRoot),
  })).toMatchObject({ status: 'success' })
  const importedFirst = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(firstMediaPath),
  })
  const importedSecond = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(secondMediaPath),
  })
  expect(importedFirst).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  expect(importedSecond).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  const firstAssetId = importedFirst.data?.id as string
  const secondAssetId = importedSecond.data?.id as string

  const firstClip = await invoke<MainTrackClip>('main_track_append_full', { assetId: firstAssetId })
  const secondClip = await invoke<MainTrackClip>('main_track_append_full', { assetId: secondAssetId })
  expect(firstClip).toMatchObject({ status: 'success', revision: expect.any(Number) })
  expect(secondClip).toMatchObject({ status: 'success', revision: expect.any(Number) })
  expect(await invoke('canvas_set', {
    canvas: {
      width: 320,
      height: 180,
      background: '#000000',
      fit: 'contain',
      position_x: 0,
      position_y: 0,
      scale: 1,
      rotation_degrees: 0,
      opacity: 1,
    },
  })).toMatchObject({ status: 'success' })

  await transcribeAndWait(firstAssetId)
  await transcribeAndWait(secondAssetId)
  const firstTranscript = await invoke<TranscriptView>('transcript_get', { assetId: firstAssetId })
  const secondTranscript = await invoke<TranscriptView>('transcript_get', { assetId: secondAssetId })
  expect(firstTranscript.data?.words.length).toBeGreaterThanOrEqual(3)
  expect(secondTranscript.data?.words.length).toBeGreaterThanOrEqual(3)
  const omitted = await invoke<{ id: string }>('edit_omit', {
    assetId: firstAssetId,
    startOrdinal: 1,
    endOrdinal: 1,
  })
  expect(omitted).toMatchObject({ status: 'success', revision: expect.any(Number) })

  const revisionBeforePreview = (await invoke<number>('project_revision')).data
  const xmemlPath = join(temporaryRoot, 'electron-rough-cut.xml')
  const assPath = join(temporaryRoot, 'electron-rough-cut.ass')
  const mp4Path = join(temporaryRoot, 'electron-rough-cut.mp4')
  const cliXmemlPath = join(temporaryRoot, 'cli-reference.xml')
  const preview = await invoke<ProjectExportPreview>('project_export_preview')
  expect(preview).toMatchObject({
    status: 'success',
    data: {
      timeline: {
        schema_version: 2,
        name: 'project Rough Cut',
        rate: 'fps_25',
      },
      subtitle_cues: expect.arrayContaining([
        expect.objectContaining({ text: expect.any(String) }),
      ]),
      compatibility: [
        expect.objectContaining({ target: 'Premiere Pro (XMEML)' }),
        expect.objectContaining({ target: 'DaVinci Resolve (XMEML)' }),
      ],
    },
    outputs: [],
  })
  expect(preview.data?.timeline.sources).toEqual(expect.arrayContaining([
    expect.objectContaining({ asset_id: firstAssetId, rate: 'fps_25' }),
    expect.objectContaining({ asset_id: secondAssetId, rate: 'fps_30_ntsc' }),
  ]))
  expect(preview.data?.timeline.clips.some((clip) => clip.source_asset_id === firstAssetId)).toBe(true)
  expect(preview.data?.timeline.clips.some((clip) => clip.source_asset_id === secondAssetId)).toBe(true)
  expect(preview.data?.subtitle_cues.length).toBeGreaterThan(0)
  expect(preview.data?.compatibility.every((report) =>
    report.preserved.length > 0 && report.limitations.length > 0)).toBe(true)
  expect((await invoke<number>('project_revision')).data).toBe(revisionBeforePreview)
  expect(readdirSync(join(projectRoot, '.doublelove', 'exports'))).toEqual([])
  for (const path of [xmemlPath, assPath, mp4Path]) expect(existsSync(path)).toBe(false)

  const xmemlGrant = await exportGrant(xmemlPath, 'xml')
  const xmeml = await invoke<ProjectExportPreview>('project_export_xmeml_apply', {
    grantToken: xmemlGrant,
  })
  expectArtifact(xmeml, 'premiere_resolve_xmeml', xmemlPath)
  const xmemlText = readFileSync(xmemlPath, 'utf8')
  expect(xmemlText).toContain('<!DOCTYPE xmeml>')
  expect(xmemlText.match(/<pathurl>/gu)).toHaveLength(2)
  for (const sourcePath of [firstMediaPath, secondMediaPath]) {
    const expectedPathUrl = pathToFileURL(realpathSync(sourcePath)).href
    expect(xmemlText).toContain(`<pathurl>${expectedPathUrl}</pathurl>`)
  }

  const replay = await rawInvoke<ProjectExportPreview>('project_export_xmeml_apply', {
    grantToken: xmemlGrant,
  })
  expect(replay).toMatchObject({ status: 'error', error: { code: 'INVALID_GRANT' } })

  const cliReference = cliExportXmeml(cliXmemlPath)
  expect(cliReference.status).toBe('success')
  expect(cliReference.outputs[0]).toMatchObject({
    kind: 'premiere_resolve_xmeml',
    path: cliXmemlPath,
    sha256: expect.stringMatching(/^[a-f0-9]{64}$/u),
  })
  expect(readFileSync(cliXmemlPath)).toEqual(readFileSync(xmemlPath))

  const ass = await invoke<ProjectExportPreview>('project_export_ass_apply', {
    grantToken: await exportGrant(assPath, 'ass'),
  })
  expectArtifact(ass, 'ass', assPath)
  const assText = readFileSync(assPath, 'utf8')
  expect(assText).toContain('[V4+ Styles]')
  expect(assText).toContain('Style: DoubleLove')
  expect(assText).toContain('开拍')

  const mp4 = await invoke<ProjectExportPreview>('project_render_mp4_apply', {
    grantToken: await exportGrant(mp4Path, 'mp4'),
  })
  expectArtifact(mp4, 'mp4_burned_subtitles', mp4Path)
  expect(statSync(mp4Path).size).toBeGreaterThan(10_000)
  const probe = JSON.parse(execFileSync(process.env.DOUBLELOVE_FFPROBE || 'ffprobe', [
    '-v', 'error',
    '-select_streams', 'v:0',
    '-show_entries', 'stream=codec_name:format=duration',
    '-of', 'json',
    mp4Path,
  ], { encoding: 'utf8' })) as {
    streams: Array<{ codec_name: string }>
    format: { duration: string }
  }
  expect(probe.streams[0]?.codec_name).toBe('h264')
  expect(Number.parseFloat(probe.format.duration)).toBeGreaterThan(2)

  const history = await invoke<Array<{ operation: string }>>('project_history', { limit: 50 })
  expect(history.status).toBe('success')
  const operations = history.data?.map((entry) => entry.operation) ?? []
  expect(operations).toEqual(expect.arrayContaining(['export_xmeml', 'export_ass', 'export_mp4']))

  const database = join(projectRoot, '.doublelove', 'project.sqlite')
  const ledger = exportLedger(database)
  for (const expected of [
    { kind: 'premiere_resolve_xmeml', path: xmemlPath, sha256: sha256(xmemlPath) },
    { kind: 'premiere_resolve_xmeml', path: cliXmemlPath, sha256: sha256(cliXmemlPath) },
    { kind: 'ass', path: assPath, sha256: sha256(assPath) },
    { kind: 'mp4_burned_subtitles', path: mp4Path, sha256: sha256(mp4Path) },
  ]) {
    expect(ledger).toContainEqual(expected)
  }

  expect(await invoke('edit_restore', {
    operationId: omitted.data?.id,
    startOrdinal: 1,
    endOrdinal: 1,
  })).toMatchObject({ status: 'success' })
  for (const [assetId, transcript] of [
    [firstAssetId, firstTranscript],
    [secondAssetId, secondTranscript],
  ] as const) {
    expect(await invoke('edit_omit', {
      assetId,
      startOrdinal: 0,
      endOrdinal: (transcript.data?.words.length as number) - 1,
    })).toMatchObject({ status: 'success' })
  }

  const emptyPath = join(temporaryRoot, 'empty-cut.xml')
  const empty = await invoke<ProjectExportPreview>('project_export_xmeml_apply', {
    grantToken: await exportGrant(emptyPath, 'xml'),
  })
  expect(empty.status).toBe('failed')
  expect(empty.diagnostics).toEqual(expect.arrayContaining([
    expect.objectContaining({ code: 'TIMELINE_EMPTY', blocks_export: true }),
  ]))
  expect(empty.outputs).toEqual([])
  expect(existsSync(emptyPath)).toBe(false)
})
