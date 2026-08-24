import * as electronAdapter from "./electron";
import * as previewAdapter from "./preview";

export * from "./normalize";

type PlatformAdapter = Omit<
  typeof electronAdapter,
  "isDesktop" | "platformKind"
> & {
  readonly isDesktop: boolean;
  readonly platformKind: "electron" | "preview";
};

const hasElectronBridge = "doubleLove" in window;

if (["file:", "dl-app:"].includes(location.protocol) && !hasElectronBridge) {
  throw new Error(
    "Double Love Studio packaged renderer started without a desktop bridge",
  );
}

const electronPlatform: PlatformAdapter = electronAdapter;
const previewPlatform: PlatformAdapter = {
  ...electronAdapter,
  ...previewAdapter,
};
const adapter: PlatformAdapter = hasElectronBridge
  ? electronPlatform
  : previewPlatform;

export const {
  isDesktop,
  platformKind,
  listen,
  projectOpen,
  createProject,
  recentProjectOpen,
  trashProject,
  projectCheckpoint,
  projectClose,
  onPrepareQuit,
  assetsList,
  importMedia,
  removeMediaAsset,
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
  mainTrackInsertAssets,
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
  grantDroppedMedia,
  playerSetBounds,
  playerLoadTimeline,
  playerSetSubtitle,
  playerSetPresentation,
  playerPlay,
  playerPause,
  playerSeek,
  playerDispose,
  onPlayerState,
  pickSavePath,
  pickProjectExportPath,
  settingsOpen,
  getAppInfo,
  updateCheck,
  updateDownload,
  updateInstall,
  preferencesGet,
  preferencesUpdate,
  historyLimitPreview,
  recentProjectsList,
  recentProjectForget,
  systemProfile,
  modelCatalog,
  modelQueueGet,
  modelInstall,
  modelPause,
  modelResume,
  modelCancel,
  modelVerify,
  modelRemove,
  modelLegacyCleanupPreview,
  modelLegacyCleanupApply,
  modelImportFolder,
  modelReveal,
  doctorRun,
  diagnosticsRevealLogs,
  onboardingGet,
  onboardingComplete,
  onboardingReset,
  editUndo,
  editRedo,
} = adapter;
