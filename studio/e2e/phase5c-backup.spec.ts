import {
  accessSync,
  constants,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { execFileSync } from 'node:child_process'
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
const cliBinary = resolve(studioRoot, '../target/debug/double-love')
const mainEntry = resolve(studioRoot, 'out/main/index.js')
const preferencesFixture = resolve(
  studioRoot,
  '../crates/double-love-desktop-service/tests/fixtures/preferences/v1.json',
)

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

let electronApp: ElectronApplication | undefined
let page: Page
let temporaryRoot: string
let userData: string
let projectRoot: string
let seededPreferences: Buffer

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(cliBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

async function launch(): Promise<void> {
  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [studioRoot, '--double-love-e2e', `--double-love-e2e-user-data=${userData}`],
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

async function openProject(): Promise<InvokeOperation<{ project_id: string; revision: number }>> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickDirectory({
      title: '选择项目目录',
      kind: 'project-open',
      e2ePath,
    }),
    projectRoot,
  )
  expect(grant?.token).toBeTruthy()
  return invoke('project_open', { grantToken: grant?.token })
}

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-electron-phase5c-e2e-'))
  userData = join(temporaryRoot, 'user-data')
  projectRoot = join(temporaryRoot, 'project')
  mkdirSync(userData, { recursive: true })
  seededPreferences = readFileSync(preferencesFixture)
  writeFileSync(join(userData, 'preferences.json'), seededPreferences)
  const seededProject = JSON.parse(execFileSync(
    cliBinary,
    ['--json', '--project', projectRoot, 'project-create'],
    { encoding: 'utf8' },
  )) as InvokeOperation<{ project_id: string }>
  expect(seededProject.status).toBe('success')
  await launch()
})

test.afterAll(async () => {
  await electronApp?.close()
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true })
})

test('backs up Tauri preferences and project data once across Electron launches', async () => {
  const preferences = await invoke<{ theme: string }>('preferences_get')
  expect(preferences).toMatchObject({ status: 'success', data: { theme: 'dark' } })
  const opened = await openProject()
  expect(opened).toMatchObject({ status: 'success', data: { revision: 0 } })

  const preferencesBackup = join(userData, 'preferences.json.pre-electron-backup')
  const projectBackup = join(
    projectRoot,
    '.doublelove',
    'project.pre-electron-backup.sqlite',
  )
  const firstPreferencesBackup = readFileSync(preferencesBackup)
  const firstProjectBackup = readFileSync(projectBackup)
  expect(firstPreferencesBackup.equals(seededPreferences)).toBe(true)
  expect(statSync(preferencesBackup).mode & 0o777).toBe(0o600)

  const preferenceChange = await invoke<{ theme: string }>('preferences_update', {
    patch: { theme: 'light' },
  })
  expect(preferenceChange).toMatchObject({ status: 'success', data: { theme: 'light' } })
  const projectChange = await invoke('canvas_set', {
    canvas: {
      width: 1280,
      height: 720,
      background: '#112233',
      fit: 'cover',
      position_x: 0,
      position_y: 0,
      scale: 1,
      rotation_degrees: 0,
      opacity: 1,
    },
  })
  expect(projectChange).toMatchObject({ status: 'success', revision: 1 })

  await electronApp?.close()
  electronApp = undefined
  await launch()
  expect(await invoke<{ theme: string }>('preferences_get')).toMatchObject({
    status: 'success',
    data: { theme: 'light' },
  })
  expect(await openProject()).toMatchObject({ status: 'success', data: { revision: 1 } })
  expect(readFileSync(preferencesBackup).equals(firstPreferencesBackup)).toBe(true)
  expect(readFileSync(projectBackup).equals(firstProjectBackup)).toBe(true)
})
