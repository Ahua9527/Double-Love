// @vitest-environment node

import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

describe('electron-builder hardening configuration', () => {
  it('loads the afterPack hook and declares the approved packaging policy', () => {
    const packageMetadata = JSON.parse(readFileSync(resolve(studioRoot, 'package.json'), 'utf8')) as { version: string }
    expect(packageMetadata.version).toBe('0.2.0')
    const config = readFileSync(resolve(studioRoot, 'electron-builder.yml'), 'utf8')
    for (const expected of [
      'appId: space.ahua.doublelove.studio',
      'productName: Double Love Studio',
      'asar: true',
      'output: release',
      '- out/**',
      'afterPack: scripts/after-pack.cjs',
      'target: dmg',
      'target: zip',
      'category: public.app-category.video',
      "minimumSystemVersion: '15.0'",
      'hardenedRuntime: true',
      'notarize: false',
      'icon: build/icon.png',
      'forceCodeSigning: false',
      'from: ../target/release/double-love-desktop-host',
      'to: double-love-desktop-host',
      'from: ../bindings/host-protocol/schema',
      'to: bindings/host-protocol/schema',
      'from: build/runtime',
      'to: runtime',
      'from: build/model-runtime',
      'to: model-runtime',
      'provider: github',
      'owner: Ahua9527',
      'repo: Double-Love',
      'releaseType: release',
    ]) {
      expect(config).toContain(expected)
    }
    expect(config.match(/^\s+- arm64$/gmu)).toHaveLength(2)

    const require = createRequire(import.meta.url)
    expect(require(resolve(studioRoot, 'scripts/after-pack.cjs'))).toEqual(expect.any(Function))
  })

  it('declares every approved fuse value in the hook', () => {
    const hook = readFileSync(resolve(studioRoot, 'scripts/after-pack.cjs'), 'utf8')
    expect(hook).toContain('[FuseV1Options.RunAsNode]: false')
    expect(hook).toContain('[FuseV1Options.EnableCookieEncryption]: true')
    expect(hook).toContain('[FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false')
    expect(hook).toContain('[FuseV1Options.EnableNodeCliInspectArguments]: false')
    expect(hook).toContain('[FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true')
    expect(hook).toContain('[FuseV1Options.OnlyLoadAppFromAsar]: true')
    expect(hook).toContain('[FuseV1Options.LoadBrowserProcessSpecificV8Snapshot]: true')
    expect(hook).toContain('[FuseV1Options.GrantFileProtocolExtraPrivileges]: false')
    expect(hook).toContain('[FuseV1Options.WasmTrapHandlers]: true')
    expect(hook).toContain('strictlyRequireAllFuses: true')
  })
})
