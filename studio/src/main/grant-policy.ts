import type { GrantKind, InvalidGrant, PathGrants } from './path-grants'
import { invalidGrant, isInvalidGrant } from './path-grants'

export interface GrantPolicy {
  kind: GrantKind
  destination: 'path' | 'target_path' | 'patch.model_root'
}

const POLICIES: Readonly<Record<string, GrantPolicy>> = Object.freeze({
  import_media: { kind: 'import-media', destination: 'path' },
  project_open: { kind: 'project-open', destination: 'path' },
  project_create: { kind: 'project-open', destination: 'path' },
  project_export_xmeml_apply: { kind: 'export-save', destination: 'target_path' },
  project_export_ass_apply: { kind: 'export-save', destination: 'target_path' },
  project_render_mp4_apply: { kind: 'export-save', destination: 'target_path' },
  export_roughcut_apply: { kind: 'export-save', destination: 'target_path' },
  preferences_update: { kind: 'model-root', destination: 'patch.model_root' },
})

export function grantPolicyFor(name: string): GrantPolicy | undefined {
  return POLICIES[name]
}

export type ApplyGrantResult =
  | { ok: true; payload: unknown }
  | { ok: false; error: InvalidGrant }

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function applyGrantPolicy(
  grants: PathGrants,
  name: string,
  payload: unknown,
): ApplyGrantResult {
  const policy = grantPolicyFor(name)
  if (!policy) return { ok: true, payload }

  if (!isRecord(payload)) return { ok: false, error: invalidGrant() }

  if (policy.destination === 'patch.model_root') {
    const patch = payload.patch
    if (!isRecord(patch)) return { ok: true, payload }

    const declaresModelRoot = 'model_root' in patch || 'grantToken' in payload || 'grantToken' in patch
    if (!declaresModelRoot) return { ok: true, payload }

    const token = payload.grantToken ?? patch.grantToken ?? patch.model_root
    const consumed = grants.consume(token, policy.kind)
    if (isInvalidGrant(consumed)) return { ok: false, error: consumed }

    const nextPatch: Record<string, unknown> = { ...patch, model_root: consumed }
    delete nextPatch.grantToken
    const nextPayload: Record<string, unknown> = { ...payload, patch: nextPatch }
    delete nextPayload.grantToken
    return { ok: true, payload: nextPayload }
  }

  const consumed = grants.consume(payload.grantToken, policy.kind)
  if (isInvalidGrant(consumed)) return { ok: false, error: consumed }

  const nextPayload = { ...payload, [policy.destination]: consumed }
  delete nextPayload.grantToken
  return { ok: true, payload: nextPayload }
}
