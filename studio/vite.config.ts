// studio/vite.config.ts —— Studio 桌面界面（Electron + React）独立工作区

/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    // 固定独立浏览器预览端口，避免与 Web 工作区冲突
    port: 5174,
    strictPort: true,
    // 允许读取仓库根的 bindings/（ts-rs 从 Rust 契约生成的 TS 类型）
    fs: { allow: ['..'] },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    setupFiles: ['src/renderer/test/setup.ts'],
  },
})
