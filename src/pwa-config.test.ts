// @vitest-environment node
import { afterAll, describe, expect, it } from 'vitest'
import { build } from 'vite'
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pwaOptions } from '../vite.config'

let generatedDirectory = ''

afterAll(async () => {
  if (generatedDirectory) await rm(generatedDirectory, { recursive: true, force: true })
})

describe('Vite PWA contract', () => {
  it('源配置使用 zh-CN 清单和由用户确认的更新模式', () => {
    expect(pwaOptions.registerType).toBe('prompt')
    expect(pwaOptions.injectRegister).toBe(false)
    expect(pwaOptions.manifest).toMatchObject({
      lang: 'zh-CN',
      display: 'standalone',
      start_url: '/?source=pwa',
      scope: '/',
    })
  })

  it('从源配置生成 zh-CN manifest 和 service worker，不读取或修改 dist', async () => {
    generatedDirectory = await mkdtemp(join(tmpdir(), 'double-love-pwa-'))
    await build({
      configFile: resolve(process.cwd(), 'vite.config.ts'),
      build: {
        outDir: generatedDirectory,
        emptyOutDir: true,
        sourcemap: false,
      },
    })

    const manifest = JSON.parse(
      await readFile(join(generatedDirectory, 'manifest.webmanifest'), 'utf8')
    ) as Record<string, unknown>
    expect(manifest).toMatchObject({
      lang: 'zh-CN',
      display: 'standalone',
      start_url: '/?source=pwa',
      scope: '/',
    })
    await expect(stat(join(generatedDirectory, 'sw.js'))).resolves.toMatchObject({
      isFile: expect.any(Function),
    })

    const headers = await readFile(join(generatedDirectory, '_headers'), 'utf8')
    expect(headers).toContain("Content-Security-Policy: default-src 'self'")
    expect(headers).toContain("frame-ancestors 'none'")
    expect(headers).toContain('X-Frame-Options: DENY')
    expect(headers).toContain('Permissions-Policy: camera=(), microphone=(), geolocation=()')
  })
})
