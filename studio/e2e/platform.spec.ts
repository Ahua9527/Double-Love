import { accessSync, constants, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'
import { test, expect, _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import './api-types'

const require = createRequire(import.meta.url)
const electronExecutable = require('electron') as string
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const hostBinary = resolve(studioRoot, '../target/debug/double-love-desktop-host')
const mainEntry = resolve(studioRoot, 'out/main/index.js')

let electronApp: ElectronApplication
let mainPage: Page
let temporaryUserData: string
let fixtureDirectory: string
let mediaPath: string
let exportPath: string

function requireBuildArtifacts(): void {
  try {
    accessSync(hostBinary, constants.X_OK)
    accessSync(mainEntry, constants.R_OK)
  } catch {
    throw new Error('Electron E2E requires a built desktop host and pnpm --dir studio electron:build')
  }
}

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryUserData = mkdtempSync(join(tmpdir(), 'double-love-electron-platform-e2e-'))
  fixtureDirectory = mkdtempSync(join(tmpdir(), 'double-love-electron-platform-fixture-'))
  mediaPath = join(fixtureDirectory, 'fixture.mov')
  exportPath = join(fixtureDirectory, 'result.xml')
  writeFileSync(mediaPath, 'synthetic media fixture')

  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      '--double-love-e2e',
      `--double-love-e2e-user-data=${temporaryUserData}`,
    ],
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  })
  mainPage = await electronApp.firstWindow()
  await mainPage.waitForLoadState('domcontentloaded')
})

test.afterAll(async () => {
  if (electronApp) await electronApp.close()
  if (temporaryUserData) rmSync(temporaryUserData, { recursive: true, force: true })
  if (fixtureDirectory) rmSync(fixtureDirectory, { recursive: true, force: true })
})

test('uses opaque, one-time, kind-bound path grants through the full host chain', async () => {
  const result = await mainPage.evaluate(async ({ media, project }) => {
    const mediaGrant = await window.doubleLove.dialogs.pickMediaFile({ e2ePath: media })
    if (!mediaGrant) throw new Error('expected media grant')
    const first = await window.doubleLove.invoke('import_media', { grantToken: mediaGrant.token })
    const replay = await window.doubleLove.invoke('import_media', { grantToken: mediaGrant.token })

    const projectGrant = await window.doubleLove.dialogs.pickDirectory({
      title: 'Open synthetic project',
      kind: 'project-open',
      e2ePath: project,
    })
    if (!projectGrant) throw new Error('expected project grant')
    const wrongKind = await window.doubleLove.invoke('import_media', { grantToken: projectGrant.token })
    const missing = await window.doubleLove.invoke('import_media', {})

    return { mediaGrant, projectGrant, first, replay, wrongKind, missing }
  }, { media: mediaPath, project: fixtureDirectory })

  expect(result.mediaGrant).toEqual({ token: expect.any(String) })
  expect(result.projectGrant).toEqual({ token: expect.any(String) })
  expect(result.first).toMatchObject({
    status: 'ok',
    result: {
      type: 'invoke',
      data: {
        status: 'failed',
        diagnostics: [expect.objectContaining({ code: 'PROJECT_NOT_OPEN' })],
      },
    },
  })
  expect(result.replay).toMatchObject({ status: 'error', error: { code: 'INVALID_GRANT' } })
  expect(result.wrongKind).toMatchObject({ status: 'error', error: { code: 'INVALID_GRANT' } })
  expect(result.missing).toMatchObject({ status: 'error', error: { code: 'INVALID_GRANT' } })
})

test('dialog overrides return tokens and never renderer-visible paths', async () => {
  const values = await mainPage.evaluate(async ({ directory, media, target }) => {
    return Promise.all([
      window.doubleLove.dialogs.pickDirectory({
        title: 'Models',
        kind: 'model-root',
        e2ePath: directory,
      }),
      window.doubleLove.dialogs.pickMediaFile({ e2ePath: media }),
      window.doubleLove.dialogs.pickExportPath({
        defaultName: 'result.xml',
        kind: 'xml',
        e2ePath: target,
      }),
    ])
  }, { directory: fixtureDirectory, media: mediaPath, target: exportPath })

  for (const value of values) {
    expect(value).toEqual({ token: expect.any(String) })
    expect(value).not.toHaveProperty('path')
  }
})

test('returns 404 for an unresolved dl-media asset', async () => {
  const result = await electronApp.evaluate(async ({ net }) => {
    const response = await net.fetch(`dl-media://asset/${crypto.randomUUID()}`)
    return {
      status: response.status,
      acceptRanges: response.headers.get('accept-ranges'),
      body: await response.text(),
    }
  })
  expect(result).toEqual({ status: 404, acceptRanges: 'bytes', body: '' })
})

test('restricts event subscriptions and returns unsubscribe for allowed events', async () => {
  const result = await mainPage.evaluate(() => {
    let disallowedRejected = false
    try {
      window.doubleLove.onEvent('dl://not-allowed', () => undefined)
    } catch {
      disallowedRejected = true
    }
    const unsubscribe = window.doubleLove.onEvent('dl://progress', () => undefined)
    const allowedType = typeof unsubscribe
    unsubscribe()
    return { disallowedRejected, allowedType }
  })
  expect(result).toEqual({ disallowedRejected: true, allowedType: 'function' })
})

test('returns an operation failure for media commands without an open project', async () => {
  const response = await mainPage.evaluate(() =>
    window.doubleLove.invoke('assets_list', {}))
  expect(response).toMatchObject({
    status: 'ok',
    result: {
      type: 'invoke',
      data: {
        status: 'failed',
        diagnostics: [expect.objectContaining({ code: 'PROJECT_NOT_OPEN' })],
      },
    },
  })
})

test('blocks renderer access to main-only host commands', async () => {
  const response = await mainPage.evaluate(() =>
    window.doubleLove.invoke('resolve_media_asset', { asset_id: 'probe' }))
  expect(response).toMatchObject({ status: 'error', error: { code: 'IPC_FORBIDDEN' } })
})
