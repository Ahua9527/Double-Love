import { randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join, resolve } from 'node:path'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { once } from 'node:events'
import { fileURLToPath, pathToFileURL } from 'node:url'
import Ajv2020, { type ValidateFunction } from 'ajv/dist/2020.js'
import {
  app,
  BrowserWindow,
  dialog,
  Menu,
  protocol,
  session,
  type IpcMainInvokeEvent,
  type MenuItemConstructorOptions,
} from 'electron'
import type { HostRequest } from '../../../bindings/host-protocol/HostRequest'
import type { HostResponse } from '../../../bindings/host-protocol/HostResponse'
import { applyGrantPolicy } from './grant-policy'
import { registerGuardedHandler, type GuardedIpcOptions, type WindowRole } from './ipc-guard'
import { LocalLog } from './local-log'
import { mediaNotFound, mediaResponse } from './media-response'
import { PathGrants, type GrantKind } from './path-grants'

protocol.registerSchemesAsPrivileged([{
  scheme: 'dl-media',
  privileges: {
    standard: false,
    secure: true,
    supportFetchAPI: true,
    stream: true,
    bypassCSP: false,
  },
}])

const PROTOCOL_VERSION = 1
const MAX_FRAME_BYTES = 64 * 1024 * 1024
const REQUEST_TIMEOUT_MS = 5_000
const SHUTDOWN_TIMEOUT_MS = 1_500
const SETTINGS_QUERY = 'window=settings'
const E2E_SWITCH = 'double-love-e2e'
const E2E_USER_DATA_SWITCH = 'double-love-e2e-user-data'
const BOTH_WINDOWS: readonly WindowRole[] = ['main', 'settings']

// Renderer-reachable host commands. Excludes main-only resolution helpers and the
// dead/internal Tauri commands that never enter Electron (capability-matrix §1).
const RENDERER_COMMANDS: ReadonlySet<string> = new Set([
  'settings_open',
  'preferences_get',
  'preferences_update',
  'recent_projects_list',
  'recent_project_forget',
  'system_profile',
  'onboarding_get',
  'onboarding_complete',
  'onboarding_reset',
  'model_catalog',
  'model_install',
  'model_pause',
  'model_resume',
  'model_cancel',
  'model_verify',
  'model_remove',
  'model_reveal',
  'doctor_run',
  'diagnostics_reveal_logs',
  'project_create',
  'project_open',
  'import_media',
  'assets_list',
  'transcribe_start',
  'task_cancel',
  'project_revision',
  'project_history',
  'project_restore_revision',
  'edit_undo',
  'edit_redo',
  'transcript_get',
  'edit_omit',
  'edit_restore',
  'roughcut_preview',
  'export_roughcut_apply',
  'project_export_preview',
  'project_export_xmeml_apply',
  'project_export_ass_apply',
  'project_render_mp4_apply',
  'timeline_get',
  'main_track_append',
  'main_track_append_full',
  'main_track_list',
  'main_track_move',
  'main_track_trim',
  'main_track_split',
  'main_track_remove',
  'canvas_get',
  'canvas_set',
  'output_rate_get',
  'output_rate_set',
  'subtitle_style_get',
  'subtitle_style_set',
  'apply_default_subtitle_style',
  'speaker_list',
  'speaker_name_proposals',
  'speaker_agent_payload_preview',
  'speaker_name_confirm',
  'speaker_merge_confirm',
  'speaker_diarize_start',
  'speaker_diarization_get',
])

const moduleDirectory = dirname(fileURLToPath(import.meta.url))
const rendererHtml = resolve(moduleDirectory, '../renderer/index.html')
const preloadPath = resolve(moduleDirectory, '../preload/index.cjs')
const repositoryRoot = resolve(moduleDirectory, '../../..')
const e2eUserData = app.isPackaged ? undefined : app.commandLine.getSwitchValue(E2E_USER_DATA_SWITCH)
const userDataPath = e2eUserData || join(app.getPath('appData'), 'space.ahua.doublelove.studio')

// Preserve the Tauri identifier-based Application Support location before any
// session, window, store, or host is created. E2E supplies an isolated override.
app.setPath('userData', userDataPath)

interface PendingRequest {
  resolve: (response: HostResponse) => void
  reject: (error: Error) => void
  timer: NodeJS.Timeout
}

interface HostEventFrame {
  v: 1
  event: string
  payload: unknown
}

function isHostEventFrame(value: unknown): value is HostEventFrame {
  return typeof value === 'object'
    && value !== null
    && (value as { v?: unknown }).v === PROTOCOL_VERSION
    && typeof (value as { event?: unknown }).event === 'string'
    && !('id' in value)
    && 'payload' in value
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

  constructor(
    private readonly log: LocalLog,
    private readonly broadcastEvent: (event: string, payload: unknown) => void,
  ) {
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

    this.log.write({ level: 'info', process: 'host', method: 'lifecycle.start', status: 'start' })
    const startedAt = performance.now()
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
      const wasStopping = this.stopping
      const detail = wasStopping
        ? 'Desktop host stopped'
        : `Desktop host exited unexpectedly (${code ?? signal ?? 'unknown'})`
      this.markUnhealthy(new Error(detail))
      this.child = null
      if (!wasStopping) {
        this.log.writeCrashMarker(code, signal)
        this.log.write({
          level: 'error',
          process: 'host',
          method: 'lifecycle.exit',
          status: 'crash',
          errorCode: 'HOST_EXIT',
        })
      }
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
    this.log.clearCrashMarker()
    this.log.write({
      level: 'info',
      process: 'host',
      method: 'handshake',
      durationMs: performance.now() - startedAt,
      status: 'ok',
    })
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

  invoke(name: string, payload: unknown): Promise<HostResponse> {
    if (!this.healthy || !this.capabilities.includes('invoke')) {
      return Promise.reject(new Error('Desktop host invoke capability is unavailable'))
    }
    return this.request({
      v: PROTOCOL_VERSION,
      id: randomUUID(),
      method: 'invoke',
      name,
      payload,
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
    this.log.write({ level: 'info', process: 'host', method: 'lifecycle.stop', status: 'stop' })
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

    if (isHostEventFrame(value)) {
      this.broadcastEvent(value.event, value.payload)
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

const grants = new PathGrants()
const log = new LocalLog(app.getPath('userData'))
const usePackagedRenderer = app.isPackaged || app.commandLine.hasSwitch(E2E_SWITCH)
const allowE2eDialogOverride = !app.isPackaged && app.commandLine.hasSwitch(E2E_SWITCH)
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

function roleForSender(event: IpcMainInvokeEvent): WindowRole | null {
  if (mainWindow && !mainWindow.isDestroyed() && event.sender === mainWindow.webContents) return 'main'
  if (settingsWindow && !settingsWindow.isDestroyed() && event.sender === settingsWindow.webContents) {
    return 'settings'
  }
  return null
}

function guardOptions(allowedWindows: readonly WindowRole[] = BOTH_WINDOWS): GuardedIpcOptions {
  return {
    allowedWindows,
    expectedUrl: isExpectedNavigation,
    roleForSender,
    onOutcome: (outcome) => log.write({
      level: outcome.status === 'ok' ? 'info' : 'warn',
      process: 'main',
      method: outcome.channel,
      durationMs: outcome.durationMs,
      status: outcome.status,
      ...(outcome.errorCode ? { errorCode: outcome.errorCode } : {}),
    }),
  }
}

function parentForEvent(event: IpcMainInvokeEvent): BrowserWindow {
  const parent = BrowserWindow.fromWebContents(event.sender)
  if (!parent) throw new Error('IPC_FORBIDDEN: Dialog sender has no application window')
  return parent
}

function e2ePathFrom(payload: Record<string, unknown>): string | null {
  if (!allowE2eDialogOverride || typeof payload.e2ePath !== 'string') return null
  if (!isAbsolute(payload.e2ePath) || payload.e2ePath.length === 0) {
    throw new Error('INVALID_PARAMS: e2ePath must be an absolute path')
  }
  return payload.e2ePath
}

function grantResponse(path: string, kind: GrantKind): { token: string } {
  return { token: grants.create(path, kind).token }
}

function invalidHostResponse(code: string, message: string): HostResponse {
  return {
    v: PROTOCOL_VERSION,
    id: randomUUID(),
    status: 'error',
    error: { code, message },
  }
}

function installIpcHandlers(): void {
  registerGuardedHandler('dl:host-health', guardOptions(), () => host?.health())
  registerGuardedHandler('app:open-settings', guardOptions(), () => openSettings())

  registerGuardedHandler('dl:dialog-pick-directory', guardOptions(), async (event, value) => {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      throw new Error('INVALID_PARAMS: Directory dialog options are required')
    }
    const payload = value as Record<string, unknown>
    const kind = payload.kind
    if (kind !== 'project-open' && kind !== 'model-root') {
      throw new Error('INVALID_PARAMS: Unsupported directory grant kind')
    }
    if (typeof payload.title !== 'string' || payload.title.length === 0 || payload.title.length > 200) {
      throw new Error('INVALID_PARAMS: Dialog title is invalid')
    }
    const override = e2ePathFrom(payload)
    if (override) return grantResponse(override, kind)

    const result = await dialog.showOpenDialog(parentForEvent(event), {
      title: payload.title,
      properties: ['openDirectory'],
    })
    return result.canceled || result.filePaths.length === 0
      ? null
      : grantResponse(result.filePaths[0], kind)
  })

  registerGuardedHandler('dl:dialog-pick-media-file', guardOptions(), async (event, value) => {
    const payload = typeof value === 'object' && value !== null && !Array.isArray(value)
      ? value as Record<string, unknown>
      : {}
    const override = e2ePathFrom(payload)
    if (override) return grantResponse(override, 'import-media')

    const result = await dialog.showOpenDialog(parentForEvent(event), {
      title: '选择要导入的媒体文件',
      properties: ['openFile'],
      filters: [{ name: '视频', extensions: ['mp4', 'mov', 'm4v', 'webm'] }],
    })
    return result.canceled || result.filePaths.length === 0
      ? null
      : grantResponse(result.filePaths[0], 'import-media')
  })

  registerGuardedHandler('dl:dialog-pick-export-path', guardOptions(), async (event, value) => {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      throw new Error('INVALID_PARAMS: Export dialog options are required')
    }
    const payload = value as Record<string, unknown>
    const kind = payload.kind
    if (kind !== 'xml' && kind !== 'ass' && kind !== 'mp4') {
      throw new Error('INVALID_PARAMS: Unsupported export kind')
    }
    if (
      typeof payload.defaultName !== 'string'
      || payload.defaultName.length === 0
      || payload.defaultName.length > 255
      || basename(payload.defaultName) !== payload.defaultName
    ) {
      throw new Error('INVALID_PARAMS: Export defaultName is invalid')
    }
    const override = e2ePathFrom(payload)
    if (override) return grantResponse(override, 'export-save')

    const exportFilterName = { xml: 'Premiere / Resolve XML', ass: 'ASS 字幕', mp4: '带字幕 MP4' }[kind]
    const result = await dialog.showSaveDialog(parentForEvent(event), {
      title: `导出 ${exportFilterName}`,
      defaultPath: payload.defaultName,
      filters: [{ name: exportFilterName, extensions: [kind] }],
    })
    return result.canceled || !result.filePath
      ? null
      : grantResponse(result.filePath, 'export-save')
  })

  registerGuardedHandler('dl:invoke', guardOptions(), async (_event, name, payload) => {
    if (typeof name !== 'string' || !/^[a-z][a-z0-9_]{0,127}$/u.test(name)) {
      return invalidHostResponse('INVALID_PARAMS', 'Command name is invalid')
    }
    if (!RENDERER_COMMANDS.has(name)) {
      return invalidHostResponse('IPC_FORBIDDEN', 'Command is not exposed to the renderer')
    }

    const startedAt = performance.now()
    const granted = applyGrantPolicy(grants, name, payload)
    if (!granted.ok) {
      log.write({
        level: 'warn',
        process: 'main',
        method: 'ipc.invoke',
        durationMs: performance.now() - startedAt,
        status: 'error',
        errorCode: granted.error.code,
      })
      return invalidHostResponse(granted.error.code, granted.error.message)
    }

    try {
      const response = await host?.invoke(name, granted.payload)
      if (!response) return invalidHostResponse('HOST_UNAVAILABLE', 'Desktop host is unavailable')
      log.write({
        level: response.status === 'ok' ? 'info' : 'warn',
        process: 'main',
        requestId: response.id,
        method: 'ipc.invoke',
        durationMs: performance.now() - startedAt,
        status: response.status,
        ...(response.status === 'error' ? { errorCode: response.error.code } : {}),
      })
      return response
    } catch {
      log.write({
        level: 'error',
        process: 'main',
        method: 'ipc.invoke',
        durationMs: performance.now() - startedAt,
        status: 'error',
        errorCode: 'HOST_UNAVAILABLE',
      })
      return invalidHostResponse('HOST_UNAVAILABLE', 'Desktop host is unavailable')
    }
  })
}

function broadcastHostEvent(event: string, payload: unknown): void {
  for (const window of [mainWindow, settingsWindow]) {
    if (window && !window.isDestroyed()) window.webContents.send('dl:host-event', event, payload)
  }
}

function extractResolvedPath(response: HostResponse): string | null {
  if (response.status !== 'ok' || response.result.type !== 'invoke') return null
  const data = response.result.data
  if (typeof data === 'string') return data
  if (typeof data !== 'object' || data === null) return null
  const record = data as Record<string, unknown>
  if (typeof record.path === 'string') return record.path
  if (typeof record.data === 'string') return record.data
  if (typeof record.data === 'object' && record.data !== null) {
    const nested = record.data as Record<string, unknown>
    if (typeof nested.path === 'string') return nested.path
  }
  return null
}

function installMediaProtocol(): void {
  protocol.handle('dl-media', async (request) => {
    let assetId: string
    try {
      const url = new URL(request.url)
      const segments = url.pathname.split('/').filter(Boolean)
      if (url.hostname !== 'asset' || segments.length !== 1) return mediaNotFound()
      assetId = decodeURIComponent(segments[0])
      if (assetId.length === 0) return mediaNotFound()
    } catch {
      return mediaNotFound()
    }

    try {
      const response = await host?.invoke('resolve_media_asset', { asset_id: assetId })
      if (!response) return mediaNotFound()
      const path = extractResolvedPath(response)
      if (!path) {
        log.write({
          level: 'warn',
          process: 'protocol',
          method: 'resolve_media_asset',
          status: 'error',
          ...(response.status === 'error' ? { errorCode: response.error.code } : {}),
        })
        return mediaNotFound()
      }
      return mediaResponse(request.method, request.headers.get('range'), path)
    } catch {
      log.write({
        level: 'error',
        process: 'protocol',
        method: 'resolve_media_asset',
        status: 'error',
        errorCode: 'PROTOCOL_ERROR',
      })
      return mediaNotFound()
    }
  })
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

    session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
      const csp = usePackagedRenderer
        ? "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob: dl-media:; font-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
        : "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob: dl-media:; font-src 'self' data:; connect-src 'self' ws:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
      callback({
        responseHeaders: {
          ...details.responseHeaders,
          'Content-Security-Policy': [csp],
        },
      })
    })

    host = new HostSupervisor(log, broadcastHostEvent)
    await host.start()
    installMenu()
    installIpcHandlers()
    installMediaProtocol()
    mainWindow = createMainWindow()

    app.on('activate', () => {
      if (!mainWindow) mainWindow = createMainWindow()
      else {
        mainWindow.show()
        mainWindow.focus()
      }
    })
  }).catch(() => {
    log.write({
      level: 'error',
      process: 'main',
      method: 'startup',
      status: 'error',
      errorCode: 'STARTUP_FAILED',
    })
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
