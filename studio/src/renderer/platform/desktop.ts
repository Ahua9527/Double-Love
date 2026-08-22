import * as electronAdapter from './electron'
import * as previewAdapter from './preview'
import * as tauriAdapter from './tauri'

export * from './normalize'

type PlatformAdapter = Omit<typeof tauriAdapter, 'isDesktop' | 'platformKind'> & {
  readonly isDesktop: boolean
  readonly platformKind: 'electron' | 'tauri' | 'preview'
}

const hasElectronBridge = 'doubleLove' in window
const hasTauriBridge = '__TAURI_INTERNALS__' in window

if (['file:', 'dl-app:'].includes(location.protocol) && !hasElectronBridge && !hasTauriBridge) {
  throw new Error('Double Love Studio packaged renderer started without a desktop bridge')
}

const electronPlatform: PlatformAdapter = electronAdapter
const previewPlatform: PlatformAdapter = previewAdapter
const adapter: PlatformAdapter = hasElectronBridge
  ? electronPlatform
  : hasTauriBridge
    ? tauriAdapter
    : previewPlatform

export const {
  isDesktop,
  platformKind,
  listen,
  projectOpen,
  projectCreate,
  assetsList,
  importMedia,
  transcriptGet,
  transcribeStart,
  taskCancel,
  projectRevision,
  projectHistory,
  projectRestoreRevision,
  editOmit,
  editRestore,
  roughcutPreview,
  exportRoughcutApply,
  timelineGet,
  mainTrackList,
  mainTrackAppendFull,
  mainTrackMove,
  mainTrackTrim,
  mainTrackSplit,
  mainTrackRemove,
  canvasGet,
  canvasSet,
  outputRateGet,
  outputRateSet,
  subtitleStyleGet,
  subtitleStyleSet,
  applyDefaultSubtitleStyle,
  speakerList,
  speakerDiarizeStart,
  speakerDiarizationGet,
  speakerNameProposals,
  speakerAgentPayloadPreview,
  speakerNameConfirm,
  speakerMergeConfirm,
  projectExportPreview,
  projectExportXmemlApply,
  projectExportAssApply,
  projectRenderMp4Apply,
  pickDirectory,
  pickMediaFile,
  pickSavePath,
  pickProjectExportPath,
  settingsOpen,
  preferencesGet,
  preferencesUpdate,
  recentProjectsList,
  recentProjectForget,
  systemProfile,
  modelCatalog,
  modelInstall,
  modelPause,
  modelResume,
  modelCancel,
  modelVerify,
  modelRemove,
  modelReveal,
  doctorRun,
  diagnosticsRevealLogs,
  onboardingGet,
  onboardingComplete,
  onboardingReset,
  editUndo,
  editRedo,
} = adapter
