export interface QuitFlowState {
  allowQuit: boolean
  quitInProgress: boolean
  installingUpdate: boolean
}

export interface QuitFlowHost {
  stop(): Promise<void>
  stopImmediately(): void
}

export interface BeforeQuitEvent {
  preventDefault(): void
}

export function createQuitFlowState(): QuitFlowState {
  return {
    allowQuit: false,
    quitInProgress: false,
    installingUpdate: false,
  }
}

export function handleBeforeQuit(
  event: BeforeQuitEvent,
  state: QuitFlowState,
  host: QuitFlowHost | null,
  quit: () => void,
  prepare?: () => Promise<void>,
): void {
  if (state.allowQuit) return

  if (state.installingUpdate) {
    state.allowQuit = true
    host?.stopImmediately()
    return
  }

  event.preventDefault()
  if (state.quitInProgress) return
  state.quitInProgress = true
  const shutdown = (prepare ? prepare() : Promise.resolve()).then(() =>
    host ? host.stop() : Promise.resolve(),
  )
  const finishQuit = () => {
    state.allowQuit = true
    quit()
  }
  void shutdown.then(finishQuit, finishQuit)
}
