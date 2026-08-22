import * as electronAdapter from './electron'
import * as previewAdapter from './preview'

export * from './normalize'

type PlatformAdapter = Omit<typeof electronAdapter, 'isDesktop' | 'platformKind'> & {
  readonly isDesktop: boolean
  readonly platformKind: 'electron' | 'preview'
}

const hasElectronBridge = 'doubleLove' in window

if (['file:', 'dl-app:'].includes(location.protocol) && !hasElectronBridge) {
  throw new Error('Double Love Studio packaged renderer started without a desktop bridge')
}

const electronPlatform: PlatformAdapter = electronAdapter
const previewPlatform: PlatformAdapter = { ...electronAdapter, ...previewAdapter }
const adapter: PlatformAdapter = hasElectronBridge ? electronPlatform : previewPlatform

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
  getAppInfo,
  updateCheck,
  updateDownload,
  updateInstall,
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
