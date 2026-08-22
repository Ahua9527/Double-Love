// @vitest-environment node

import { describe, expect, it } from 'vitest'
import { applyGrantPolicy, grantPolicyFor } from './grant-policy'
import { PathGrants } from './path-grants'

describe('grant command policies', () => {
  it.each([
    ['import_media', 'import-media', 'path'],
    ['project_open', 'project-open', 'path'],
    ['project_create', 'project-open', 'path'],
    ['project_export_xmeml_apply', 'export-save', 'target_path'],
    ['project_export_ass_apply', 'export-save', 'target_path'],
    ['project_render_mp4_apply', 'export-save', 'target_path'],
    ['export_roughcut_apply', 'export-save', 'target_path'],
    ['preferences_update', 'model-root', 'patch.model_root'],
  ] as const)('maps %s to %s', (command, kind, destination) => {
    expect(grantPolicyFor(command)).toEqual({ kind, destination })
  })

  it('injects a consumed path and removes the token', () => {
    const grants = new PathGrants()
    const { token } = grants.create('/private/source.mov', 'import-media')

    expect(applyGrantPolicy(grants, 'import_media', { grantToken: token, keep: true })).toEqual({
      ok: true,
      payload: { path: '/private/source.mov', keep: true },
    })
  })

  it('injects model_root inside the preferences patch', () => {
    const grants = new PathGrants()
    const { token } = grants.create('/private/models', 'model-root')

    expect(applyGrantPolicy(grants, 'preferences_update', {
      grantToken: token,
      patch: { model_root: true, theme: 'dark' },
    })).toEqual({
      ok: true,
      payload: { patch: { model_root: '/private/models', theme: 'dark' } },
    })
  })

  it('requires grants for declared write commands', () => {
    const grants = new PathGrants()
    expect(applyGrantPolicy(grants, 'project_open', {})).toMatchObject({
      ok: false,
      error: { code: 'INVALID_GRANT' },
    })
  })

  it('passes unknown read-only policies through untouched', () => {
    const grants = new PathGrants()
    const payload = { assetId: 'asset-1' }
    expect(applyGrantPolicy(grants, 'transcript_get', payload)).toEqual({ ok: true, payload })
  })
})
