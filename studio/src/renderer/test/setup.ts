// jsdom 环境缺少的浏览器 API 桩 + 每个浏览器用例后自动卸载组件

import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

if (typeof window !== 'undefined') {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }),
  })

  // vitest 未开 globals，testing-library 的自动清理不会注册，这里手动挂上
  afterEach(() => {
    cleanup()
  })
}
