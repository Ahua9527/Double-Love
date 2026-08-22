import { randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { once } from 'node:events'
import { fileURLToPath, pathToFileURL } from 'node:url'
import Ajv2020, { type ValidateFunction } from 'ajv/dist/2020.js'
import {
  app,
  BrowserWindow,
  ipcMain,
  Menu,
  session,
  type MenuItemConstructorOptions,
} from 'electron'
import type { HostRequest } from '../../../bindings/host-protocol/HostRequest'
import type { HostResponse } from '../../../bindings/host-protocol/HostResponse'

const PROTOCOL_VERSION = 1
const MAX_FRAME_BYTES = 64 * 1024 * 1024
const REQUEST_TIMEOUT_MS = 5_000
const SHUTDOWN_TIMEOUT_MS = 1_500
const SETTINGS_QUERY = 'window=settings'
const E2E_SWITCH = 'double-love-e2e'
const E2E_USER_DATA_SWITCH = 'double-love-e2e-user-data'

const moduleDirectory = dirname(fileURLToPath(import.meta.url))
const rendererHtml = resolve(moduleDirectory, '../renderer/index.html')
const preloadPath = resolve(moduleDirectory, '../preload/index.cjs')
const repositoryRoot = resolve(moduleDirectory, '../../..')
const e2eUserData = app.commandLine.getSwitchValue(E2E_USER_DATA_SWITCH)
const userDataPath = e2eUserData || join(app.getPath('appData'), 'space.ahua.doublelove.studio')

// Preserve the Tauri identifier-based Application Support location before any
// session, window, store, or host is created. E2E supplies an isolated override.
app.setPath('userData', userDataPath)

interface PendingRequest {
  resolve: (response: HostResponse) => void
  reject: (error: Error) => void
  timer: NodeJS.Timeout
}

class HostSupervisor {
  private child: ChildProcessWithoutNullStreams | null = null
  private buffer = Buffer.alloc(0)
  private expectedFrameLength: number | null = null
  private pending = new Map<string, PendingRequest>()
  private healthy = false
  private stopping = false
  private capabilities: readonly string[] = []
  private readonly validateResponse: ValidateFunction<HostResponse>

  constructor() {
    const schemaRoot = app.isPackaged
      ? join(process.resourcesPath, 'bindings/host-protocol/schema')
      : join(repositoryRoot, 'bindings/host-protocol/schema')
    const responseSchema = JSON.parse(
      readFileSync(join(schemaRoot, 'HostResponse.schema.json'), 'utf8'),
    ) as object
    const ajv = new Ajv2020({ allErrors: true, strict: true })
    ajv.addFormat('uint32', {
      type: 'number',
      validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= 0xffffffff,
    })
    this.validateResponse = ajv.compile<HostResponse>(responseSchema)
  }

  async start(): Promise<void> {
    if (this.child) return

    const hostPath = app.isPackaged
      ? join(process.resourcesPath, 'double-love-desktop-host')
      : join(repositoryRoot, 'target/debug/double-love-desktop-host')

    this.stopping = false
    const child = spawn(hostPath, ['--app-data-dir', app.getPath('userData')], {
      shell: false,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    this.child = child
    child.stdout.on('data', (chunk: Buffer) => this.onStdout(chunk))
    child.stderr.resume()
    child.once('error', (error) => this.markUnhealthy(error))
    child.once('exit', (code, signal) => {
      const detail = this.stopping
        ? 'Desktop host stopped'
        : `Desktop host exited unexpectedly (${code ?? signal ?? 'unknown'})`
      this.markUnhealthy(new Error(detail))
      this.child = null
    })

    const response = await this.request({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      method: 'handshake',
      client: 'electron-main',
      client_protocol: PROTOCOL_VERSION,
    })
    if (
      response.status !== 'ok'
      || response.result.type !== 'hello'
      || response.result.data.protocol !== PROTOCOL_VERSION
    ) {
      throw new Error('Desktop host handshake returned an incompatible response')
    }
    this.capabilities = Object.freeze([...response.result.data.capabilities])
    this.healthy = true
  }

  health(): Promise<HostResponse> {
    if (!this.healthy || !this.capabilities.includes('health')) {
      return Promise.reject(new Error('Desktop host is unhealthy'))
    }
    return this.request({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      method: 'health',
    })
  }

  async stop(): Promise<void> {
    const child = this.child
    if (!child) return

    this.stopping = true
    if (this.healthy) {
      try {
        await Promise.race([
          (async () => {
            await this.request({
              v: PROTOCOL_VERSION,
              id: randomUUID(),
              method: 'shutdown',
            })
            if (child.exitCode === null && child.signalCode === null) await once(child, 'exit')
          })(),
          new Promise<never>((_, reject) => {
            setTimeout(() => reject(new Error('Desktop host shutdown timed out')), SHUTDOWN_TIMEOUT_MS)
          }),
        ])
      } catch {
        // The hard kill below is the required fallback for failed shutdown.
      }
    }

    if (child.exitCode === null && child.signalCode === null) child.kill()
    this.healthy = false
  }

  private request(request: HostRequest): Promise<HostResponse> {
    const child = this.child
    if (!child?.stdin.writable) return Promise.reject(new Error('Desktop host is not running'))

    const payload = Buffer.from(JSON.stringify(request), 'utf8')
    if (payload.byteLength > MAX_FRAME_BYTES) {
      return Promise.reject(new Error('Desktop host request exceeds the frame limit'))
    }
    const header = Buffer.allocUnsafe(4)
    header.writeUInt32BE(payload.byteLength)

    return new Promise<HostResponse>((resolveRequest, rejectRequest) => {
      const timer = setTimeout(() => {
        this.pending.delete(request.id)
        rejectRequest(new Error(`Desktop host request timed out: ${request.method}`))
      }, REQUEST_TIMEOUT_MS)
      this.pending.set(request.id, { resolve: resolveRequest, reject: rejectRequest, timer })
      child.stdin.write(Buffer.concat([header, payload]), (error) => {
        if (!error) return
        const pending = this.pending.get(request.id)
        if (!pending) return
        clearTimeout(pending.timer)
        this.pending.delete(request.id)
        pending.reject(error)
      })
    })
  }

  private onStdout(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk])

    while (true) {
      if (this.expectedFrameLength === null) {
        if (this.buffer.byteLength < 4) return
        this.expectedFrameLength = this.buffer.readUInt32BE(0)
        this.buffer = this.buffer.subarray(4)
        if (this.expectedFrameLength > MAX_FRAME_BYTES) {
          this.markUnhealthy(new Error('Desktop host response exceeds the frame limit'))
          this.child?.kill()
          return
        }
      }

      if (this.buffer.byteLength < this.expectedFrameLength) return
      const frame = this.buffer.subarray(0, this.expectedFrameLength)
      this.buffer = this.buffer.subarray(this.expectedFrameLength)
      this.expectedFrameLength = null
      this.handleFrame(frame)
    }
  }

  private handleFrame(frame: Buffer): void {
    let value: unknown
    try {
      value = JSON.parse(frame.toString('utf8'))
    } catch {
      this.markUnhealthy(new Error('Desktop host returned invalid JSON'))
      this.child?.kill()
      return
    }

    if (!this.validateResponse(value)) {
      this.markUnhealthy(new Error('Desktop host response failed protocol schema validation'))
      this.child?.kill()
      return
    }

    const pending = this.pending.get(value.id)
    if (!pending) return
    clearTimeout(pending.timer)
    this.pending.delete(value.id)
    pending.resolve(value)
  }

  private markUnhealthy(error: Error): void {
    this.healthy = false
    this.capabilities = []
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer)
      pending.reject(error)
    }
    this.pending.clear()
  }
}

let mainWindow: BrowserWindow | null = null
let settingsWindow: BrowserWindow | null = null
let host: HostSupervisor | null = null
let allowQuit = false
let quitInProgress = false

const usePackagedRenderer = app.isPackaged || app.commandLine.hasSwitch(E2E_SWITCH)
const developmentRendererUrl = 'http://localhost:5174'

function isExpectedNavigation(target: string): boolean {
  try {
    const url = new URL(target)
    if (!usePackagedRenderer) return url.origin === new URL(developmentRendererUrl).origin
    return url.protocol === 'file:' && url.pathname === pathToFileURL(rendererHtml).pathname
  } catch {
    return false
  }
}

function secureWindow(window: BrowserWindow): void {
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  window.webContents.on('will-navigate', (event, target) => {
    if (!isExpectedNavigation(target)) event.preventDefault()
  })
  window.webContents.on('before-input-event', (event, input) => {
    if (input.type.toLowerCase().includes('keydown') && input.meta && input.key === ',') {
      event.preventDefault()
      openSettings()
    }
  })
}

function windowOptions(): Electron.BrowserWindowConstructorOptions {
  return {
    title: 'Double Love Studio',
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      webviewTag: false,
    },
  }
}

async function loadRenderer(window: BrowserWindow, settings = false): Promise<void> {
  if (!usePackagedRenderer) {
    const suffix = settings ? `?${SETTINGS_QUERY}` : ''
    await window.loadURL(`${developmentRendererUrl}${suffix}`)
    return
  }
  await window.loadFile(rendererHtml, settings ? { query: { window: 'settings' } } : undefined)
}

function createMainWindow(): BrowserWindow {
  const window = new BrowserWindow({
    ...windowOptions(),
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
  })
  secureWindow(window)
  window.once('closed', () => {
    mainWindow = null
  })
  void loadRenderer(window)
  return window
}

function openSettings(): void {
  if (settingsWindow && !settingsWindow.isDestroyed()) {
    settingsWindow.show()
    settingsWindow.focus()
    return
  }

  const window = new BrowserWindow({
    ...windowOptions(),
    width: 760,
    height: 580,
    minWidth: 700,
    minHeight: 500,
  })
  settingsWindow = window
  secureWindow(window)
  window.on('close', (event) => {
    if (allowQuit) return
    event.preventDefault()
    window.hide()
  })
  window.once('closed', () => {
    settingsWindow = null
  })
  void loadRenderer(window, true)
}

function installMenu(): void {
  const template: MenuItemConstructorOptions[] = [
    {
      label: 'Double Love Studio',
      submenu: [
        {
          id: 'settings',
          label: '设置…',
          accelerator: 'Cmd+,',
          click: openSettings,
        },
        { type: 'separator' },
        { role: 'quit' },
      ],
    },
    {
      label: '编辑',
      submenu: [
        { role: 'undo' },
        { role: 'redo' },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'selectAll' },
      ],
    },
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

const hasSingleInstanceLock = app.requestSingleInstanceLock()
if (!hasSingleInstanceLock) {
  app.quit()
} else {
  app.on('second-instance', () => {
    if (!mainWindow) return
    if (mainWindow.isMinimized()) mainWindow.restore()
    mainWindow.show()
    mainWindow.focus()
  })

  app.whenReady().then(async () => {
    session.defaultSession.setPermissionRequestHandler((_webContents, _permission, callback) => {
      callback(false)
    })

    host = new HostSupervisor()
    await host.start()
    installMenu()
    ipcMain.handle('dl:host-health', () => host?.health())
    ipcMain.handle('app:open-settings', () => openSettings())
    mainWindow = createMainWindow()

    app.on('activate', () => {
      if (!mainWindow) mainWindow = createMainWindow()
      else {
        mainWindow.show()
        mainWindow.focus()
      }
    })
  }).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : 'Desktop startup failed')
    app.quit()
  })
}

app.on('window-all-closed', () => {
  app.quit()
})

app.on('before-quit', (event) => {
  if (allowQuit) return
  event.preventDefault()
  if (quitInProgress) return
  quitInProgress = true
  const shutdown = host ? host.stop() : Promise.resolve()
  void shutdown.finally(() => {
    allowQuit = true
    app.quit()
  })
})
