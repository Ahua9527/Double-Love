import { accessSync, constants, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
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
const corruptFixture = resolve(
  studioRoot,
  '../crates/double-love-desktop-service/tests/fixtures/preferences/corrupt.json',
)

interface InvokeOperation<T> {
  status: 'success' | 'partial' | 'failed' | 'cancelled'
  data: T | null
  diagnostics: Array<{ code: string }>
}

interface InvokeEnvelope<T> {
  status: 'ok' | 'error'
  result?: { type: string; data?: InvokeOperation<T> }
  error?: { code: string }
}

let electronApp: ElectronApplication
let page: Page
let userData: string

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

async function launch(): Promise<void> {
  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      '--double-love-e2e',
      `--double-love-e2e-user-data=${userData}`,
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

test.beforeAll(async () => {
  requireBuildArtifacts()
  userData = mkdtempSync(join(tmpdir(), 'double-love-electron-slice1-e2e-'))
  await launch()
})

test.afterAll(async () => {
  if (electronApp) await electronApp.close()
  if (userData) rmSync(userData, { recursive: true, force: true })
})

test('persists preferences across restart and covers recovery and validation paths', async () => {
  const defaults = await invoke<{
    schema_version: number
    theme: string
    model_root: string
    model_endpoint: string
    onboarding_completed: boolean
  }>('preferences_get')
  expect(defaults.status).toBe('success')
  expect(defaults.data).toMatchObject({
    schema_version: 1,
    theme: 'light',
    model_endpoint: 'https://huggingface.co',
    onboarding_completed: false,
  })
  expect(defaults.data?.model_root).toBe(join(userData, 'models'))

  const mutation = await page.evaluate(async () => {
    let changed: unknown
    const observed = new Promise<unknown>((resolveEvent) => {
      const unsubscribe = window.doubleLove.onEvent('dl://preferences-changed', (payload) => {
        changed = payload
        unsubscribe()
        resolveEvent(payload)
      })
    })
    const response = await window.doubleLove.invoke('preferences_update', { patch: { theme: 'dark' } })
    await observed
    return { response, changed }
  }) as { response: InvokeEnvelope<{ theme: string }>; changed: unknown }
  expect(mutation.response.result?.data?.status).toBe('success')
  expect(mutation.response.result?.data?.data?.theme).toBe('dark')
  expect(mutation.changed).toEqual({ changed_keys: ['theme'] })

  const preferencesPath = join(userData, 'preferences.json')
  expect(JSON.parse(readFileSync(preferencesPath, 'utf8'))).toMatchObject({
    app_preferences: { schema_version: 1, theme: 'dark' },
  })

  await electronApp.close()
  await launch()
  const persisted = await invoke<{ theme: string }>('preferences_get')
  expect(persisted.data?.theme).toBe('dark')

  await electronApp.close()
  writeFileSync(preferencesPath, readFileSync(corruptFixture))
  await launch()
  await expect.poll(() => readdirSync(userData).some((name) => name.startsWith('preferences.corrupt.'))).toBe(true)
  await page.waitForTimeout(500)

  // The renderer app shell may perform the first recovery read during startup. Replant the same
  // synthetic corrupt fixture so the renderer-backed invoke itself proves the warning contract.
  writeFileSync(preferencesPath, readFileSync(corruptFixture))
  const recovered = await invoke<{ theme: string }>('preferences_get')
  expect(recovered.status).toBe('success')
  expect(recovered.diagnostics.map((diagnostic) => diagnostic.code)).toContain('PREFERENCES_RECOVERED')
  expect(readdirSync(userData).some((name) => name.startsWith('preferences.corrupt.'))).toBe(true)

  const invalidEndpoint = await invoke('preferences_update', {
    patch: { model_endpoint: 'http://example.test/models' },
  })
  expect(invalidEndpoint.status).toBe('failed')
  expect(invalidEndpoint.diagnostics[0]?.code).toBe('MODEL_ENDPOINT_INVALID')

  const reset = await invoke<{ completed: boolean; step: number }>('onboarding_reset')
  expect(reset).toMatchObject({ status: 'success', data: { completed: false, step: 1 } })
  const preferencesAfterReset = await invoke<{ onboarding_completed: boolean }>('preferences_get')
  expect(preferencesAfterReset.data?.onboarding_completed).toBe(false)

  const profile = await invoke<{ architecture: string; recommended_asr_model: string }>('system_profile')
  expect(profile.status).toBe('success')
  expect(profile.data?.architecture).toBe('arm64')
  expect(['qwen3-asr-0.6b', 'qwen3-asr-1.7b']).toContain(profile.data?.recommended_asr_model)
})
