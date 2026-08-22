import { ipcMain, type IpcMainInvokeEvent } from 'electron'

export type WindowRole = 'main' | 'settings'

export interface GuardedIpcOutcome {
  channel: string
  durationMs: number
  status: 'ok' | 'error'
  errorCode?: 'IPC_FORBIDDEN' | 'INVALID_PARAMS' | 'IPC_HANDLER_ERROR'
}

export interface GuardedIpcOptions {
  allowedWindows: readonly WindowRole[]
  expectedUrl: (url: string) => boolean
  roleForSender: (event: IpcMainInvokeEvent) => WindowRole | null
  maxPayloadBytes?: number
  onOutcome?: (outcome: GuardedIpcOutcome) => void
}

export class IpcGuardError extends Error {
  constructor(readonly code: 'IPC_FORBIDDEN' | 'INVALID_PARAMS', message: string) {
    super(`${code}: ${message}`)
    this.name = 'IpcGuardError'
  }
}

const DEFAULT_MAX_PAYLOAD_BYTES = 8 * 1024 * 1024

function serializedSize(args: readonly unknown[]): number {
  try {
    const serialized = JSON.stringify(args)
    return Buffer.byteLength(serialized ?? 'null', 'utf8')
  } catch {
    throw new IpcGuardError('INVALID_PARAMS', 'Payload must be JSON serializable')
  }
}

export function registerGuardedHandler(
  channel: string,
  options: GuardedIpcOptions,
  handler: (event: IpcMainInvokeEvent, ...args: unknown[]) => unknown | Promise<unknown>,
): void {
  ipcMain.handle(channel, async (event, ...args: unknown[]) => {
    const startedAt = performance.now()
    try {
      const role = options.roleForSender(event)
      const senderUrl = event.senderFrame?.url
      if (!role || !options.allowedWindows.includes(role) || !senderUrl || !options.expectedUrl(senderUrl)) {
        throw new IpcGuardError('IPC_FORBIDDEN', 'Sender is not an allowed application window')
      }
      if (serializedSize(args) > (options.maxPayloadBytes ?? DEFAULT_MAX_PAYLOAD_BYTES)) {
        throw new IpcGuardError('INVALID_PARAMS', 'Payload exceeds the IPC size limit')
      }
      const result = await handler(event, ...args)
      options.onOutcome?.({ channel, durationMs: performance.now() - startedAt, status: 'ok' })
      return result
    } catch (error) {
      const errorCode = error instanceof IpcGuardError
        ? error.code
        : error instanceof Error && error.message.startsWith('INVALID_PARAMS:')
          ? 'INVALID_PARAMS'
          : 'IPC_HANDLER_ERROR'
      options.onOutcome?.({
        channel,
        durationMs: performance.now() - startedAt,
        status: 'error',
        errorCode,
      })
      throw error
    }
  })
}
