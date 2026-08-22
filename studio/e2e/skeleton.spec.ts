import { accessSync, constants, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'
import { test, expect, _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import './api-types'

declare global {
  interface Window {
    readonly require?: unknown
    readonly process?: unknown
    readonly global?: unknown
  }
}

const require = createRequire(import.meta.url)
const electronExecutable = require('electron') as string
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const hostBinary = resolve(studioRoot, '../target/debug/double-love-desktop-host')
const mainEntry = resolve(studioRoot, 'out/main/index.js')

let electronApp: ElectronApplication
let mainPage: Page
let temporaryUserData: string

async function pressCommandComma(): Promise<string | null> {
  return electronApp.evaluate(({ BrowserWindow, Menu }) => {
    const main = BrowserWindow.getAllWindows().find((window) =>
      !window.webContents.getURL().includes('window=settings'))
    main?.webContents.sendInputEvent({ type: 'keyDown', keyCode: ',', modifiers: ['meta'] })
    main?.webContents.sendInputEvent({ type: 'keyUp', keyCode: ',', modifiers: ['meta'] })
    return Menu.getApplicationMenu()?.getMenuItemById('settings')?.accelerator ?? null
  })
}

test.beforeAll(async () => {
  try {
    accessSync(hostBinary, constants.X_OK)
  } catch {
    throw new Error(
      `Electron E2E requires the desktop host binary. Run: cargo build -p double-love-desktop-host (expected ${hostBinary})`,
    )
  }
  try {
    accessSync(mainEntry, constants.R_OK)
  } catch {
    throw new Error(
      `Electron E2E requires the built main entry. Run: pnpm --dir studio electron:build (expected ${mainEntry})`,
    )
  }

  temporaryUserData = mkdtempSync(join(tmpdir(), 'double-love-electron-e2e-'))
  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      `--double-love-e2e`,
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
})

test('runs the sandboxed window, host bridge, and settings singleton', async () => {
  await expect.poll(() => electronApp.windows().length).toBe(1)
  await expect(mainPage).toHaveTitle('Double Love Studio')

  const rendererBoundary = await mainPage.evaluate(() => ({
    requireType: typeof window.require,
    processType: typeof window.process,
    globalType: typeof window.global,
    apiFrozen: Object.isFrozen(window.doubleLove),
    dialogsFrozen: Object.isFrozen(window.doubleLove.dialogs),
    updatesFrozen: Object.isFrozen(window.doubleLove.updates),
    apiKeys: Object.keys(window.doubleLove).sort(),
    networkResources: performance.getEntriesByType('resource')
      .map((entry) => entry.name)
      .filter((name) => /^https?:/u.test(name)),
  }))
  expect(rendererBoundary).toEqual({
    requireType: 'undefined',
    processType: 'undefined',
    globalType: 'undefined',
    apiFrozen: true,
    dialogsFrozen: true,
    updatesFrozen: true,
    apiKeys: ['dialogs', 'getAppInfo', 'hostHealth', 'invoke', 'onEvent', 'openSettings', 'updates'],
    networkResources: [],
  })
  expect(await mainPage.evaluate(() => window.doubleLove.getAppInfo())).toEqual({
    name: 'Double Love Studio',
    version: '0.2.0',
  })

  const firstHealth = await mainPage.evaluate(() => window.doubleLove.hostHealth())
  expect(firstHealth).toMatchObject({
    v: 1,
    status: 'ok',
    result: { type: 'health', data: { healthy: true } },
  })

  await expect(pressCommandComma()).resolves.toBe('Cmd+,')
  await expect.poll(() => electronApp.windows().length).toBe(2)
  const settingsPage = electronApp.windows().find((page) => page !== mainPage)
  expect(settingsPage).toBeDefined()
  await expect(settingsPage!).toHaveTitle('Double Love Studio')
  await expect.poll(() => settingsPage!.url()).toContain('window=settings')

  await mainPage.evaluate(() => window.doubleLove.openSettings())
  await expect.poll(() => electronApp.windows().length).toBe(2)

  await electronApp.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows().find((window) =>
      window.webContents.getURL().includes('window=settings'))?.close()
  })
  await expect.poll(async () => electronApp.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows()
    return {
      count: windows.length,
      settingsVisible: windows.some((window) =>
        window.webContents.getURL().includes('window=settings') && window.isVisible()),
    }
  })).toEqual({ count: 2, settingsVisible: false })

  await mainPage.bringToFront()
  await expect(pressCommandComma()).resolves.toBe('Cmd+,')
  await expect.poll(async () => electronApp.evaluate(({ BrowserWindow }) => {
    const settings = BrowserWindow.getAllWindows().find((window) =>
      window.webContents.getURL().includes('window=settings'))
    return { count: BrowserWindow.getAllWindows().length, settingsVisible: settings?.isVisible() ?? false }
  })).toEqual({ count: 2, settingsVisible: true })

  const secondHealth = await mainPage.evaluate(() => window.doubleLove.hostHealth())
  expect(secondHealth).toMatchObject({
    v: 1,
    status: 'ok',
    result: { type: 'health', data: { healthy: true } },
  })
})
