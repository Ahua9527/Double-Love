import { execFileSync } from 'node:child_process'
import {
  accessSync,
  constants,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { test, expect, _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import './api-types'

const require = createRequire(import.meta.url)
const electronExecutable = require('electron') as string
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const hostBinary = resolve(studioRoot, '../target/debug/double-love-desktop-host')
const mainEntry = resolve(studioRoot, 'out/main/index.js')

interface InvokeOperation<T> {
  status: 'success' | 'partial' | 'failed' | 'cancelled'
  revision: number | null
  data: T | null
  diagnostics: Array<{ code: string; blocks_export?: boolean }>
  outputs: Array<{ kind: string; path: string; sha256: string }>
}

interface InvokeEnvelope<T> {
  status: 'ok' | 'error'
  result?: { type: string; data?: InvokeOperation<T> }
  error?: { code: string }
}

interface MediaAssetSummary {
  id: string
  status: string
}

interface SpeakerIdentity {
  id: string
  display_name: string
  confirmed: boolean
}

interface SpeakerMergeProposal {
  left_speaker_id: string
  right_speaker_id: string
  status: string
}

interface SpeakerDiarization {
  segment_count: number
  speakers: SpeakerIdentity[]
  merge_proposals: SpeakerMergeProposal[]
}

interface SpeakerAgentPayload {
  speaker_id: string
  utterances: string[]
  instruction: string
}

interface TranscriptWord {
  ordinal: number
  display_text: string
  speaker_assignments: Array<{ speaker_id: string; evidence: string }>
}

interface TranscriptView {
  words: TranscriptWord[]
}

interface CapturedEvent {
  channel: string
  payload: unknown
}

let electronApp: ElectronApplication
let page: Page
let temporaryRoot: string
let projectRoot: string
let firstMediaPath: string
let secondMediaPath: string
let userDataPath: string

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK)
  accessSync(mainEntry, constants.R_OK)
}

function generateMedia(path: string, frequency: number): void {
  execFileSync(process.env.DOUBLELOVE_FFMPEG || 'ffmpeg', [
    '-hide_banner',
    '-loglevel', 'error',
    '-y',
    '-f', 'lavfi',
    '-i', 'color=c=black:s=320x180:r=25:d=4',
    '-f', 'lavfi',
    '-i', `sine=frequency=${frequency}:sample_rate=48000:duration=4`,
    '-c:v', 'mpeg4',
    '-pix_fmt', 'yuv420p',
    '-c:a', 'aac',
    '-shortest',
    path,
  ])
}

function seedInstalledModels(userData: string): void {
  const modelRoot = join(userData, 'models')
  mkdirSync(modelRoot, { recursive: true })
  const installed = (modelId: string, revision: string) => {
    mkdirSync(join(modelRoot, modelId, revision), { recursive: true })
    return {
      model_id: modelId,
      revision,
      state: 'installed',
      bytes_downloaded: 0,
      bytes_total: 0,
      staging_id: null,
      last_error_code: null,
      last_error_message: null,
      updated_at: '2026-01-01T00:00:00Z',
    }
  }
  writeFileSync(join(modelRoot, 'installations.json'), JSON.stringify({
    schema_version: 1,
    installations: {
      'qwen3-asr-0.6b': installed(
        'qwen3-asr-0.6b',
        '5eb144179a02acc5e5ba31e748d22b0cf3e303b0',
      ),
      'qwen3-forced-aligner-0.6b': installed(
        'qwen3-forced-aligner-0.6b',
        'c7cbfc2048c462b0d63a45797104fc9db3ad62b7',
      ),
      'silero-vad': installed(
        'silero-vad',
        '806dcba3f0b5d95282d0889a074954a2f8c6397b',
      ),
      'wespeaker-zh': installed(
        'wespeaker-zh',
        'f5a201849aa7cae741ec75cd02a0bc9dd5712ca2',
      ),
    },
  }, null, 2))
}

async function invoke<T>(name: string, payload?: unknown): Promise<InvokeOperation<T>> {
  const response = await page.evaluate(
    async ({ command, commandPayload }) => window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  ) as InvokeEnvelope<T>
  expect(response.status).toBe('ok')
  expect(response.result?.type).toBe('invoke')
  return response.result?.data as InvokeOperation<T>
}

async function directoryGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickDirectory({
      title: 'Create synthetic Slice 6 project',
      kind: 'project-open',
      e2ePath,
    }),
    path,
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function mediaGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickMediaFile({ e2ePath }),
    path,
  )
  expect(grant).toEqual({ token: expect.any(String) })
  return grant?.token as string
}

async function capturedEvents(): Promise<CapturedEvent[]> {
  return page.evaluate(() => {
    const captured = window as unknown as { slice6Events: CapturedEvent[] }
    return captured.slice6Events
  })
}

async function startAndWait(name: string, payload: unknown): Promise<string> {
  const started = await invoke<{ task_id: string }>(name, payload)
  expect(started.status).toBe('success')
  const taskId = started.data?.task_id as string
  await expect.poll(async () => {
    const states = (await capturedEvents()).filter((event) => event.channel === 'dl://task-state')
    return states.find((event) =>
      (event.payload as { task_id?: string }).task_id === taskId)?.payload
  }).toMatchObject({ task_id: taskId, state: 'succeeded' })
  return taskId
}

function sqlLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`
}

function updateTranscriptText(
  database: string,
  assetId: string,
  canonicalProjectRoot: string,
  canonicalMediaPath: string,
): void {
  const activeRun = `(SELECT active_transcript_run_id FROM media_asset WHERE id=${sqlLiteral(assetId)})`
  const texts = [
    '我是李明。',
    projectRoot,
    canonicalProjectRoot,
    canonicalMediaPath,
    '乙方秘密文本',
  ]
  const statements = texts.map((text, ordinal) => `
    UPDATE transcript_word
       SET raw_text=${sqlLiteral(text)}, display_text=${sqlLiteral(text)}
     WHERE asset_id=${sqlLiteral(assetId)}
       AND run_id=${activeRun}
       AND ordinal=${ordinal};
  `).join('\n')
  execFileSync('sqlite3', ['-cmd', '.timeout 5000', database, statements])
}

function assignOtherSpeaker(database: string, assetId: string, speakerId: string): void {
  const assignment = JSON.stringify([{
    speaker_id: speakerId,
    confidence: 1,
    evidence: 'slice6_other_speaker',
  }])
  execFileSync('sqlite3', [
    '-cmd', '.timeout 5000', database,
    `UPDATE transcript_word
        SET speaker_assignments_json=${sqlLiteral(assignment)}
      WHERE asset_id=${sqlLiteral(assetId)}
        AND run_id=(SELECT active_transcript_run_id FROM media_asset WHERE id=${sqlLiteral(assetId)})
        AND ordinal=4;`,
  ])
}

function containsEmbeddingShapedArray(value: unknown): boolean {
  if (Array.isArray(value)) {
    return (value.length >= 2 && value.every((item) => typeof item === 'number'))
      || value.some(containsEmbeddingShapedArray)
  }
  if (typeof value === 'object' && value !== null) {
    return Object.values(value).some(containsEmbeddingShapedArray)
  }
  return false
}

function containsEmbeddingField(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(containsEmbeddingField)
  if (typeof value !== 'object' || value === null) return false
  return Object.entries(value).some(([key, nested]) =>
    ['embedding', 'embeddings', 'embedding_values', 'values'].includes(key)
    || containsEmbeddingField(nested))
}

function containsFloatArrayShapedString(value: unknown): boolean {
  if (typeof value === 'string') {
    return /\[\s*[+-]?(?:\d+\.\d*|\.\d+)(?:[eE][+-]?\d+)?(?:\s*,\s*[+-]?(?:\d+\.\d*|\.\d+)(?:[eE][+-]?\d+)?)+\s*\]/.test(value)
  }
  if (Array.isArray(value)) return value.some(containsFloatArrayShapedString)
  if (typeof value !== 'object' || value === null) return false
  return Object.values(value).some(containsFloatArrayShapedString)
}

function expectNoEmbeddingBoundary(value: unknown): void {
  expect(containsEmbeddingField(value)).toBe(false)
  expect(containsEmbeddingShapedArray(value)).toBe(false)
  expect(containsFloatArrayShapedString(value)).toBe(false)
}

test.beforeAll(async () => {
  requireBuildArtifacts()
  temporaryRoot = mkdtempSync(join(tmpdir(), 'double-love-electron-slice6-e2e-'))
  projectRoot = join(temporaryRoot, 'project')
  firstMediaPath = join(temporaryRoot, 'slice6-first.mp4')
  secondMediaPath = join(temporaryRoot, 'slice6-second.mp4')
  userDataPath = join(temporaryRoot, 'user-data')
  generateMedia(firstMediaPath, 440)
  generateMedia(secondMediaPath, 660)
  seedInstalledModels(userDataPath)

  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      '--double-love-e2e',
      '--double-love-e2e-transcribe-mock',
      '--double-love-e2e-speaker-mock',
      `--double-love-e2e-user-data=${userDataPath}`,
    ],
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  })
  page = await electronApp.firstWindow()
  await page.waitForLoadState('domcontentloaded')
  await page.evaluate(() => {
    const captured = window as unknown as { slice6Events: CapturedEvent[] }
    captured.slice6Events = []
    for (const channel of [
      'dl://progress',
      'dl://task-state',
      'dl://model-progress',
      'dl://model-state',
      'dl://preferences-changed',
      'dl://doctor-result',
    ]) {
      window.doubleLove.onEvent(channel, (payload) => captured.slice6Events.push({ channel, payload }))
    }
  })
  await expect.poll(async () => {
    const response = await page.evaluate(() => window.doubleLove.hostHealth())
    return (response as { status?: string }).status
  }).toBe('ok')
})

test.afterAll(async () => {
  if (electronApp) await electronApp.close()
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true })
})

test('diarizes, names, merges, and keeps speaker privacy boundaries intact', async () => {
  test.setTimeout(60_000)
  expect(await invoke('project_create', {
    grantToken: await directoryGrant(projectRoot),
  })).toMatchObject({ status: 'success' })
  const firstImport = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(firstMediaPath),
  })
  const secondImport = await invoke<MediaAssetSummary>('import_media', {
    grantToken: await mediaGrant(secondMediaPath),
  })
  expect(firstImport).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  expect(secondImport).toMatchObject({ status: 'success', data: { status: 'prepared' } })
  const firstAssetId = firstImport.data?.id as string
  const secondAssetId = secondImport.data?.id as string

  for (const assetId of [firstAssetId, secondAssetId]) {
    await startAndWait('transcribe_start', {
      assetId,
      model: 'qwen3-asr-0.6b',
      language: 'auto',
    })
  }

  const database = join(projectRoot, '.doublelove', 'project.sqlite')
  const canonicalProjectRoot = realpathSync(projectRoot)
  const canonicalFirstMediaPath = realpathSync(firstMediaPath)
  updateTranscriptText(database, firstAssetId, canonicalProjectRoot, canonicalFirstMediaPath)

  await startAndWait('speaker_diarize_start', { assetId: firstAssetId })
  const firstDiarization = await invoke<SpeakerDiarization>(
    'speaker_diarization_get',
    { assetId: firstAssetId },
  )
  expect(firstDiarization).toMatchObject({
    status: 'success',
    data: { segment_count: 1, speakers: [expect.objectContaining({ confirmed: false })] },
  })
  const firstSpeakerId = firstDiarization.data?.speakers[0].id as string

  await startAndWait('speaker_diarize_start', { assetId: secondAssetId })
  const secondDiarization = await invoke<SpeakerDiarization>(
    'speaker_diarization_get',
    { assetId: secondAssetId },
  )
  expect(secondDiarization).toMatchObject({ status: 'success', data: { segment_count: 1 } })
  const secondSpeakerId = secondDiarization.data?.speakers[0].id as string
  expect(secondSpeakerId).not.toBe(firstSpeakerId)
  expect(secondDiarization.data?.merge_proposals.some((proposal) =>
    new Set([proposal.left_speaker_id, proposal.right_speaker_id]).size === 2
    && [proposal.left_speaker_id, proposal.right_speaker_id].includes(firstSpeakerId)
    && [proposal.left_speaker_id, proposal.right_speaker_id].includes(secondSpeakerId)
    && proposal.status === 'pending')).toBe(true)

  assignOtherSpeaker(database, firstAssetId, secondSpeakerId)
  const proposals = await invoke<Array<{ speaker_id: string; candidate_name: string }>>(
    'speaker_name_proposals',
    { assetId: firstAssetId },
  )
  expect(proposals.data).toContainEqual(expect.objectContaining({
    speaker_id: firstSpeakerId,
    candidate_name: '李明',
  }))

  const agent = await invoke<SpeakerAgentPayload>('speaker_agent_payload_preview', {
    assetId: firstAssetId,
    speakerId: firstSpeakerId,
  })
  expect(agent.status).toBe('success')
  expect(agent.data?.speaker_id).toBe(firstSpeakerId)
  const agentText = JSON.stringify(agent.data)
  expect(agentText).toContain('我是李明')
  expect(agentText).toContain('<PROJECT>')
  expect(agentText).toContain('<MEDIA>')
  expect(agentText).not.toContain(projectRoot)
  expect(agentText).not.toContain(canonicalProjectRoot)
  expect(agentText).not.toContain(firstMediaPath)
  expect(agentText).not.toContain(canonicalFirstMediaPath)
  expect(agentText).not.toContain('乙方秘密文本')
  expectNoEmbeddingBoundary(agent)

  const rejectedName = await invoke('speaker_name_confirm', {
    speakerId: firstSpeakerId,
    displayName: '李明',
    confirmed: false,
  })
  expect(rejectedName).toMatchObject({
    status: 'failed',
    diagnostics: [{ code: 'SPEAKER_CONFIRM_REQUIRED' }],
  })
  const confirmedName = await invoke<SpeakerIdentity>('speaker_name_confirm', {
    speakerId: firstSpeakerId,
    displayName: '李明',
    confirmed: true,
  })
  expect(confirmedName).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: { id: firstSpeakerId, display_name: '李明', confirmed: true },
  })

  const speakersBeforeMerge = await invoke<SpeakerIdentity[]>('speaker_list')
  const transcriptBeforeMerge = await invoke<TranscriptView>('transcript_get', { assetId: firstAssetId })
  const speakerNames = new Map(speakersBeforeMerge.data?.map((speaker) => [speaker.id, speaker.display_name]))
  const transcriptSpeakerNames = transcriptBeforeMerge.data?.words
    .flatMap((word) => word.speaker_assignments.map((assignment) => speakerNames.get(assignment.speaker_id)))
  expect(transcriptSpeakerNames).toContain('李明')
  expect(transcriptBeforeMerge.data?.words.find((word) => word.ordinal === 4)?.speaker_assignments[0]?.speaker_id)
    .toBe(secondSpeakerId)

  const rejectedMerge = await invoke('speaker_merge_confirm', {
    keepSpeakerId: firstSpeakerId,
    mergeSpeakerId: secondSpeakerId,
    confirmed: false,
  })
  expect(rejectedMerge).toMatchObject({
    status: 'failed',
    diagnostics: [{ code: 'SPEAKER_CONFIRM_REQUIRED' }],
  })
  const merged = await invoke<SpeakerIdentity>('speaker_merge_confirm', {
    keepSpeakerId: firstSpeakerId,
    mergeSpeakerId: secondSpeakerId,
    confirmed: true,
  })
  expect(merged).toMatchObject({
    status: 'success',
    revision: expect.any(Number),
    data: { id: firstSpeakerId, display_name: '李明', confirmed: true },
  })
  const speakersAfterMerge = await invoke<SpeakerIdentity[]>('speaker_list')
  expect(speakersAfterMerge.data).toEqual([expect.objectContaining({ id: firstSpeakerId, display_name: '李明' })])
  const transcriptAfterMerge = await invoke<TranscriptView>('transcript_get', { assetId: firstAssetId })
  expect(transcriptAfterMerge.data?.words.find((word) => word.ordinal === 4)?.speaker_assignments[0])
    .toMatchObject({ speaker_id: firstSpeakerId, evidence: 'confirmed_speaker_merge' })

  for (const response of [
    firstDiarization,
    secondDiarization,
    proposals,
    agent,
    confirmedName,
    speakersBeforeMerge,
    transcriptBeforeMerge,
    merged,
    speakersAfterMerge,
    transcriptAfterMerge,
  ]) {
    expectNoEmbeddingBoundary(response)
  }
  const events = await capturedEvents()
  expect(events.length).toBeGreaterThan(0)
  expectNoEmbeddingBoundary(events)
  const eventText = JSON.stringify(events)
  for (const sensitivePath of [
    projectRoot,
    canonicalProjectRoot,
    firstMediaPath,
    canonicalFirstMediaPath,
    secondMediaPath,
    realpathSync(secondMediaPath),
  ]) {
    expect(eventText).not.toContain(sensitivePath)
  }
})
