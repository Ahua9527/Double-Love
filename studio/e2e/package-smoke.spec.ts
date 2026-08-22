import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test, expect, chromium, type Browser, type Page } from '@playwright/test'
import './api-types'

const packagedApp = process.env.DOUBLELOVE_PACKAGED_APP

test.skip(!packagedApp, 'Set DOUBLELOVE_PACKAGED_APP to the unpacked macOS .app path')

async function availablePort(): Promise<number> {
  const server = createServer()
  await new Promise<void>((resolveListen, rejectListen) => {
    server.once('error', rejectListen)
    server.listen(0, '127.0.0.1', resolveListen)
  })
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Failed to reserve a CDP port')
  await new Promise<void>((resolveClose, rejectClose) => {
    server.close((error) => error ? rejectClose(error) : resolveClose())
  })
  return address.port
}

async function connectToPackagedRenderer(port: number): Promise<Browser> {
  let lastError: unknown
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
    } catch (error) {
      lastError = error
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 100))
    }
  }
  throw lastError
}

async function stopPackagedApp(child: ChildProcess, browser: Browser | undefined, page: Page | undefined): Promise<void> {
  if (page && !page.isClosed()) await page.close().catch(() => undefined)
  if (browser?.isConnected()) await browser.close().catch(() => undefined)
  if (child.exitCode !== null || child.signalCode !== null) return

  child.kill('SIGTERM')
  await Promise.race([
    once(child, 'exit'),
    new Promise((resolveTimeout) => setTimeout(resolveTimeout, 2_000)),
  ])
  if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
}

test('packaged app boots from app.asar and reaches the bundled host through packaged schemas', async () => {
  const appPath = resolve(packagedApp!)
  const executablePath = join(appPath, 'Contents/MacOS/Double Love Studio')
  const userData = mkdtempSync(join(tmpdir(), 'double-love-packaged-e2e-'))
  const remoteDebuggingPort = await availablePort()
  const child = spawn(executablePath, [
    '--remote-debugging-address=127.0.0.1',
    `--remote-debugging-port=${remoteDebuggingPort}`,
    '--double-love-e2e',
    `--double-love-e2e-user-data=${userData}`,
  ], {
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  child.stdout?.resume()
  child.stderr?.resume()

  let browser: Browser | undefined
  let page: Page | undefined
  try {
    browser = await connectToPackagedRenderer(remoteDebuggingPort)
    const context = browser.contexts()[0]
    await expect.poll(() => context.pages().length).toBeGreaterThan(0)
    page = context.pages()[0]
    await page.waitForLoadState('domcontentloaded')
    await expect(page).toHaveTitle('Double Love Studio')
    expect(page.url()).toBe('dl-app://app/index.html')

    const health = await page.evaluate(() => window.doubleLove.hostHealth())
    expect(health).toMatchObject({
      v: 1,
      status: 'ok',
      result: { type: 'health', data: { healthy: true } },
    })

    const preferences = await page.evaluate(() => window.doubleLove.invoke('preferences_get'))
    expect(preferences).toMatchObject({
      v: 1,
      status: 'ok',
      result: {
        type: 'invoke',
        data: { status: 'success', data: { model_root: join(userData, 'models') } },
      },
    })
  } finally {
    await stopPackagedApp(child, browser, page)
    rmSync(userData, { recursive: true, force: true })
  }
})
