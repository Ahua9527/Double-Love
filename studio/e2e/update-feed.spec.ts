import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { expect, test, chromium, type Browser } from '@playwright/test'
import './api-types'

const packagedApp = process.env.DOUBLELOVE_PACKAGED_APP
const repositoryRoot = resolve(import.meta.dirname, '../..')

test.skip(!packagedApp, 'Set DOUBLELOVE_PACKAGED_APP to the current unpacked macOS .app path')
test.setTimeout(300_000)

interface LocalFeed {
  child: ChildProcess
  url: string
  feedDir: string
  artifact: string
  requests: string[]
}

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

async function startLocalFeed(): Promise<LocalFeed> {
  const child = spawn('bash', [join(repositoryRoot, 'scripts/migration/local-update-feed.sh')], {
    cwd: repositoryRoot,
    env: { ...process.env, CSC_IDENTITY_AUTO_DISCOVERY: 'false' },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  const requests: string[] = []
  child.stderr?.setEncoding('utf8')
  child.stderr?.on('data', (chunk: string) => requests.push(chunk))

  const ready = await new Promise<{ url: string; feedDir: string; artifact: string }>((resolveReady, rejectReady) => {
    let output = ''
    const timer = setTimeout(() => rejectReady(new Error('Local update feed build timed out')), 240_000)
    child.stdout?.setEncoding('utf8')
    child.stdout?.on('data', (chunk: string) => {
      output += chunk
      for (const line of output.split('\n')) {
        const prefix = 'DOUBLELOVE_LOCAL_UPDATE_FEED_READY='
        if (!line.startsWith(prefix)) continue
        clearTimeout(timer)
        requests.length = 0
        resolveReady(JSON.parse(line.slice(prefix.length)))
        return
      }
    })
    child.once('error', (error) => {
      clearTimeout(timer)
      rejectReady(error)
    })
    child.once('exit', (code, signal) => {
      clearTimeout(timer)
      rejectReady(new Error(`Local update feed exited before ready (${code ?? signal ?? 'unknown'})`))
    })
  })
  return { child, requests, ...ready }
}

async function connectToPackagedRenderer(port: number): Promise<Browser> {
  let lastError: unknown
  for (let attempt = 0; attempt < 80; attempt += 1) {
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
    } catch (error) {
      lastError = error
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 100))
    }
  }
  throw lastError
}

async function stopChild(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return
  child.kill('SIGTERM')
  await Promise.race([
    once(child, 'exit'),
    new Promise((resolveTimeout) => setTimeout(resolveTimeout, 3_000)),
  ])
  if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
}

async function stopPackagedApp(child: ChildProcess, browser?: Browser): Promise<void> {
  if (browser?.isConnected()) await browser.close().catch(() => undefined)
  await stopChild(child)
}

test('local generic feed proves explicit check and download without an implicit install', async () => {
  const feed = await startLocalFeed()
  const appPath = resolve(packagedApp!)
  const executablePath = join(appPath, 'Contents/MacOS/Double Love Studio')
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-update-e2e-'))
  const userData = join(temporaryRoot, 'user-data')
  const isolatedHome = join(temporaryRoot, 'home')
  mkdirSync(join(isolatedHome, 'Library/Caches'), { recursive: true })
  const remoteDebuggingPort = await availablePort()
  const appProcess = spawn(executablePath, [
    '--remote-debugging-address=127.0.0.1',
    `--remote-debugging-port=${remoteDebuggingPort}`,
    '--double-love-e2e',
    `--double-love-e2e-user-data=${userData}`,
  ], {
    env: {
      ...process.env,
      HOME: isolatedHome,
      DOUBLELOVE_UPDATE_FEED_URL: feed.url,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  appProcess.stdout?.resume()
  appProcess.stderr?.resume()

  let browser: Browser | undefined
  try {
    browser = await connectToPackagedRenderer(remoteDebuggingPort)
    const context = browser.contexts()[0]
    await expect.poll(() => context.pages().length).toBeGreaterThan(0)
    const mainPage = context.pages()[0]
    await mainPage.waitForLoadState('domcontentloaded')
    await expect(mainPage).toHaveTitle('Double Love Studio')
    expect(await mainPage.evaluate(() => window.doubleLove.getAppInfo())).toEqual({
      name: 'Double Love Studio',
      version: '0.2.0',
    })

    await expect.poll(() => feed.requests.join('').includes('latest-mac.yml'), { timeout: 30_000 }).toBe(true)
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 500))
    await mainPage.evaluate(() => window.doubleLove.openSettings())
    await expect.poll(() => context.pages().length).toBe(2)
    const settingsPage = context.pages().find((page) => page.url().includes('window=settings'))
    if (!settingsPage) throw new Error('Settings window did not open')
    await settingsPage.waitForLoadState('domcontentloaded')
    await settingsPage.getByRole('button', { name: '关于' }).click()

    await settingsPage.evaluate(() => {
      document.documentElement.dataset.updateEvents = '[]'
      window.doubleLove.onEvent('dl://update-status', (payload) => {
        const current = JSON.parse(document.documentElement.dataset.updateEvents ?? '[]')
        current.push(payload)
        document.documentElement.dataset.updateEvents = JSON.stringify(current)
      })
    })

    feed.requests.length = 0
    const checked = await settingsPage.evaluate(() => window.doubleLove.updates.check())
    expect(checked).toEqual({ stage: 'update-available', version: '0.2.1-feed' })
    await expect(settingsPage.getByText('发现新版本 0.2.1-feed')).toBeVisible()
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 500))

    const beforeDownloadEvents = await settingsPage.evaluate(() => JSON.parse(document.documentElement.dataset.updateEvents ?? '[]'))
    expect(beforeDownloadEvents.some((status: { stage?: string }) => ['download-progress', 'update-downloaded'].includes(status.stage ?? ''))).toBe(false)
    expect(feed.requests.join('')).not.toContain('.zip')

    let downloadConfirmed = false
    settingsPage.once('dialog', async (dialog) => {
      expect(dialog.message()).toContain('确认下载这个更新')
      downloadConfirmed = true
      await dialog.accept()
    })
    await settingsPage.getByRole('button', { name: '下载更新' }).click()
    await expect.poll(() => downloadConfirmed).toBe(true)
    await expect(settingsPage.getByText('更新已下载，可以重启安装。')).toBeVisible({ timeout: 120_000 })
    expect(feed.requests.join('')).toContain('.zip')

    const statuses = await settingsPage.evaluate(() => JSON.parse(document.documentElement.dataset.updateEvents ?? '[]'))
    expect(statuses).toEqual(expect.arrayContaining([
      expect.objectContaining({ stage: 'checking-update' }),
      expect.objectContaining({ stage: 'update-available', version: '0.2.1-feed' }),
      expect.objectContaining({ stage: 'download-progress', percent: 100 }),
      expect.objectContaining({ stage: 'update-downloaded', version: '0.2.1-feed' }),
    ]))
    for (const status of statuses) {
      expect(Object.keys(status).every((key) => ['stage', 'version', 'percent'].includes(key))).toBe(true)
    }
    const serializedStatuses = JSON.stringify(statuses)
    expect(serializedStatuses).not.toContain(feed.url)
    expect(serializedStatuses).not.toContain(feed.feedDir)
    expect(serializedStatuses).not.toContain(feed.artifact)
    expect(serializedStatuses).not.toContain('127.0.0.1')

    let installDismissed = false
    settingsPage.once('dialog', async (dialog) => {
      expect(dialog.message()).toContain('确认退出 Double Love Studio 并安装更新')
      installDismissed = true
      await dialog.dismiss()
    })
    await settingsPage.getByRole('button', { name: '重启安装' }).click()
    await expect.poll(() => installDismissed).toBe(true)
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 750))
    expect(appProcess.exitCode).toBeNull()
    await expect(settingsPage).toHaveTitle('Double Love Studio')
  } finally {
    await stopPackagedApp(appProcess, browser)
    await stopChild(feed.child)
    rmSync(temporaryRoot, { recursive: true, force: true })
  }
})
