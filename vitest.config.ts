// vitest.config.ts

import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [{
    name: 'vitest-pwa-register-stub',
    resolveId(id) {
      if (id === 'virtual:pwa-register/react') return '\0virtual:pwa-register/react'
    },
    load(id) {
      if (id === '\0virtual:pwa-register/react') {
        return 'export const useRegisterSW = () => ({ offlineReady: [false, () => undefined], needRefresh: [false, () => undefined], updateServiceWorker: () => undefined })'
      }
    },
  }],
  test: {
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
  },
})
