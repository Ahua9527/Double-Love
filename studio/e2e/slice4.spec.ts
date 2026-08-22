import { execFileSync } from 'node:child_process'
import { accessSync, constants, mkdtempSync, readFileSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
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
  diagnostics: Array<{ code: string }>
}

interface InvokeEnvelope<T> {
  status: 'ok' | 'error'
  result?: { type: string; data?: InvokeOperation<T> }
  error?: { code: string }
}

interface MediaAssetSummary {
  id: string
  display_name: string
  status: string
}

interface MainTrackClip {
  id: string
  source_asset_id: string
  source_in_frame: number
  source_out_frame: number
  order_index: number
}

let electronApp: ElectronApplication
let page: Page
let temporaryRoot: string
let projectRoot: string
let mediaAPath: string
let mediaBPath: string

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

function generateMedia(path: string, color: string, frequency: number): void {
  execFileSync(process.env.DOUBLELOVE_FFMPEG || 'ffmpeg', [
    '-hide_banner',
    '-loglevel', 'error',
    '-y',
    '-f', 'lavfi',
    '-i', `color=c=${color}:s=320x180:r=25:d=1`,
    '-f', 'lavfi',
    '-i', `sine=frequency=${frequency}:sample_rate=48000:duration=1`,
    '-c:v', 'mpeg4',
    '-pix_fmt', 'yuv420p',
    '-c:a', 'aac',
    '-shortest',
    path,
  ])
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
      title: 'Create synthetic Slice 4 project',
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

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-electron-slice4-e2e-'))
  projectRoot = join(temporaryRoot, 'project')
  mediaAPath = join(temporaryRoot, 'slice4-a.mp4')
  mediaBPath = join(temporaryRoot, 'slice4-b.mp4')
  generateMedia(mediaAPath, 'red', 440)
  generateMedia(mediaBPath, 'blue', 660)

  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      '--double-love-e2e',
      `--double-love-e2e-user-data=${join(temporaryRoot, 'user-data')}`,
    ],
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  })
  page = await electronApp.firstWindow()
  await page.waitForLoadState('domcontentloaded')
  await expect.poll(async () => {
    const response = await page.evaluate(() => window.doubleLove.hostHealth())
    return (response as { status?: string }).status
  }).toBe('ok')
})

test.afterAll(async () => {
  if (electronApp) await electronApp.close()
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true })
})

test('edits the main track and round-trips project visual settings without path exposure', async () => {
  const created = await invoke<{ project_id: string }>('project_create', {
    grantToken: await directoryGrant(projectRoot),
  })
  expect(created.status).toBe('success')

  const importedA = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(mediaAPath),
  })
  const importedB = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(mediaBPath),
  })
  expect(importedA).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  expect(importedB).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  const assetA = importedA.data?.id as string
  const assetB = importedB.data?.id as string

  const appendedA = await invoke<MainTrackClip>('main_track_append_full', { assetId: assetA })
  const appendedB = await invoke<MainTrackClip>('main_track_append', {
    assetId: assetB,
    sourceInFrame: 0,
    sourceOutFrame: 25,
  })
  expect(appendedA).toMatchObject({ status: 'success', revision: expect.any(Number) })
  expect(appendedB).toMatchObject({ status: 'success', revision: expect.any(Number) })
  const clipA = appendedA.data?.id as string
  const clipB = appendedB.data?.id as string

  const moved = await invoke<null>('main_track_move', { clipId: clipA, beforeClipId: null })
  expect(moved).toMatchObject({ status: 'success', revision: expect.any(Number) })
  const trimmed = await invoke<MainTrackClip>('main_track_trim', {
    clipId: clipB,
    sourceInFrame: 2,
    sourceOutFrame: 23,
  })
  expect(trimmed).toMatchObject({
    status: 'success',
    data: { source_in_frame: 2, source_out_frame: 23 },
  })
  const split = await invoke<MainTrackClip[]>('main_track_split', {
    clipId: clipB,
    sourceAtFrame: 12,
  })
  expect(split.status).toBe('success')
  expect(split.data).toHaveLength(2)
  const rightClip = split.data?.[1].id as string
  const removed = await invoke<null>('main_track_remove', { clipId: rightClip })
  expect(removed).toMatchObject({ status: 'success', revision: expect.any(Number) })

  const listed = await invoke<MainTrackClip[]>('main_track_list')
  expect(listed.status).toBe('success')
  expect(listed.data).toEqual([
    expect.objectContaining({
      id: clipB,
      source_asset_id: assetB,
      source_in_frame: 2,
      source_out_frame: 12,
      order_index: 0,
    }),
    expect.objectContaining({
      id: clipA,
      source_asset_id: assetA,
      source_in_frame: 0,
      source_out_frame: 25,
      order_index: 1,
    }),
  ])

  const timeline = await invoke<{
    schema_version: number
    name: string
    rate: string
    sources: Array<{ asset_id: string }>
    clips: Array<{ source_asset_id: string }>
  }>('timeline_get')
  expect(timeline).toMatchObject({
    status: 'success',
    data: {
      schema_version: 2,
      name: 'project Rough Cut',
      rate: 'fps_25',
    },
  })
  expect(timeline.data?.sources.map((source) => source.asset_id).sort()).toEqual([assetA, assetB].sort())
  expect(timeline.data?.clips.map((clip) => clip.source_asset_id)).toEqual([assetB, assetA])

  const canvas = {
    width: 1280,
    height: 720,
    background: '#223344',
    fit: 'cover',
    position_x: 4,
    position_y: -3,
    scale: 1.15,
    rotation_degrees: 2,
    opacity: 0.85,
  }
  expect(await invoke<typeof canvas>('canvas_set', { canvas })).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: canvas,
  })
  expect((await invoke<typeof canvas>('canvas_get')).data).toEqual(canvas)

  expect((await invoke<string | null>('output_rate_get')).data).toBeNull()
  expect(await invoke<string | null>('output_rate_set', { rate: 'fps_30' })).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: 'fps_30',
  })
  expect((await invoke<string | null>('output_rate_get')).data).toBe('fps_30')
  expect((await invoke<{ rate: string }>('timeline_get')).data?.rate).toBe('fps_30')
  expect(await invoke<string | null>('output_rate_set', { rate: null })).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: null,
  })
  expect((await invoke<string | null>('output_rate_get')).data).toBeNull()
  expect((await invoke<{ rate: string }>('timeline_get')).data?.rate).toBe('fps_25')

  const initialStyle = (await invoke<Record<string, unknown>>('subtitle_style_get')).data as Record<string, unknown>
  const projectStyle = { ...initialStyle, font_size: 60 }
  expect(await invoke<Record<string, unknown>>('subtitle_style_set', { style: projectStyle })).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: { font_size: 60 },
  })
  expect((await invoke<Record<string, unknown>>('subtitle_style_get')).data?.font_size).toBe(60)

  const defaultStyle = { ...initialStyle, font_family: 'Helvetica', font_size: 43 }
  expect(await invoke<Record<string, unknown>>('preferences_update', {
    patch: { default_subtitle_style: defaultStyle },
  })).toMatchObject({ status: 'success' })
  const applied = await invoke<Record<string, unknown>>('apply_default_subtitle_style')
  expect(applied).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: { font_family: 'Helvetica', font_size: 43 },
  })
  expect((await invoke<Record<string, unknown>>('subtitle_style_get')).data).toEqual(defaultStyle)

  const unknown = await invoke<MainTrackClip>('main_track_trim', {
    clipId: realpathSync(projectRoot),
    sourceInFrame: 0,
    sourceOutFrame: 1,
  })
  expect(unknown.status).toBe('failed')
  expect(unknown.diagnostics[0]?.code).toBe('MAIN_TRACK_CLIP_MISSING')
  expect(JSON.stringify(unknown)).not.toContain(projectRoot)
  expect(JSON.stringify(unknown)).not.toContain(realpathSync(projectRoot))
  expect(JSON.stringify(unknown)).toContain('<PROJECT>')

  const expectedMedia = readFileSync(mediaAPath)
  const mediaResponse = await electronApp.evaluate(async ({ net }, id) => {
    const response = await net.fetch(`dl-media://asset/${encodeURIComponent(id)}`)
    return {
      status: response.status,
      body: Array.from(new Uint8Array(await response.arrayBuffer())),
    }
  }, assetA)
  expect(mediaResponse).toEqual({ status: 200, body: Array.from(expectedMedia) })
  const missingMediaStatus = await electronApp.evaluate(async ({ net }) =>
    (await net.fetch(`dl-media://asset/${crypto.randomUUID()}`)).status)
  expect(missingMediaStatus).toBe(404)
})
