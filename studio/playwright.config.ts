import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  expect: { timeout: 10_000 },
  reporter: 'list',
  outputDir: join(tmpdir(), 'double-love-electron-playwright-results'),
  use: {
    trace: 'off',
    screenshot: 'off',
    video: 'off',
  },
})
