// @vitest-environment node

import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

describe('electron-builder hardening configuration', () => {
  it('loads the afterPack hook and declares the approved builder identity', () => {
    const config = readFileSync(resolve(studioRoot, 'electron-builder.yml'), 'utf8')
    expect(config).toContain('appId: space.ahua.doublelove.studio')
    expect(config).toContain('productName: Double Love Studio')
    expect(config).toContain('asar: true')
    expect(config).toContain('afterPack: scripts/after-pack.cjs')

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
  })
})
