// vite.config.ts

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'
import fs from 'fs'

// 读取package.json获取版本号
const packageJson = JSON.parse(fs.readFileSync('./package.json', 'utf-8'));
const { version } = packageJson;

export const pwaOptions = {
  registerType: 'prompt', // 更新提示模式（autoUpdate 下 onNeedRefresh 永不触发）
  injectRegister: false, // 关闭自动注入，改由 virtual:pwa-register 注册
  includeAssets: [ // 需要缓存的静态资源
    'favicon.ico',          // 传统favicon
    'apple-touch-icon.png', // iOS主屏图标
    'Double-Love_96_any.png',   // 通用小尺寸图标
    'Double-Love_192_any.png',  // 中等尺寸图标
    'Double-Love_512_any.png',  // 大尺寸图标
    'Double-Love_96_maskable.png',  // 可裁剪小图标
    'Double-Love_192_maskable.png', // 可裁剪中等图标
    'Double-Love_512_maskable.png'  // 可裁剪大图标
  ],
  // PWA应用清单配置
  manifest: {
    name: 'Double Love', // 完整应用名称
    short_name: 'Double Love', // 主屏显示短名称
    lang: 'zh-CN', // 清单语言与界面语言保持一致
    description: 'Double Love：让每个镜头都藏着我未说出口的帧率', // 应用描述
    theme_color: '#171717', // 主题色
    background_color: '#171717', // 启动背景色
    display: 'standalone', // 显示模式（独立应用）
    id: "/?source=pwa", // 应用唯一标识
    start_url: '/?source=pwa', // 启动URL
    scope: '/', // 作用域
    orientation: 'any', // 屏幕方向
    categories: ['productivity', 'utilities'], // 应用分类
    // 图标配置（适配不同平台和设备）
    icons: [
      {
        src: 'apple-touch-icon.png', // iOS专用图标
        sizes: '180x180', // 推荐尺寸
        type: 'image/png'
      },
      // 通用图标配置
      {
        src: 'Double-Love_96_any.png',
        sizes: '96x96', // 小尺寸
        type: 'image/png',
        purpose: 'any' // 通用用途
      },
      {
        src: 'Double-Love_192_any.png', // 中等尺寸
        sizes: '192x192',
        type: 'image/png',
        purpose: 'any'
      },
      {
        src: 'Double-Love_512_any.png', // 大尺寸
        sizes: '512x512',
        type: 'image/png',
        purpose: 'any'
      },
      // 可裁剪图标（适配Android等平台）
      {
        src: 'Double-Love_96_maskable.png',
        sizes: '96x96',
        type: 'image/png',
        purpose: 'maskable' // 可裁剪
      },
      {
        src: 'Double-Love_192_maskable.png',
        sizes: '192x192',
        type: 'image/png',
        purpose: 'maskable'
      },
      {
        src: 'Double-Love_512_maskable.png',
        sizes: '512x512',
        type: 'image/png',
        purpose: 'maskable'
      }
    ]
  },
  // Service Worker配置
  workbox: {
    // 缓存匹配模式
    globPatterns: [
      '**/*.{js,css,html,ico,png,svg,woff2,jpg,jpeg,gif,json,webp}' // 缓存所有静态资源
    ]
  }
} satisfies Parameters<typeof VitePWA>[0];

// 主配置对象
export default defineConfig({
  // 插件配置
  plugins: [
    react(), // 启用React支持
    
    // PWA配置
    VitePWA(pwaOptions)
  ],
  // 定义环境变量供前端使用
  define: {
    'import.meta.env.APP_VERSION': JSON.stringify(version),
    'import.meta.env.BUILD_DATE': JSON.stringify(new Date().toISOString())
  },
  // 构建配置
  build: {
    sourcemap: true, // 生成sourcemap
    rollupOptions: {
      output: {
        // 手动分包策略
        manualChunks: {
          vendor: ['react', 'react-dom'] // 将React相关包单独分包
        }
      }
    },
    chunkSizeWarningLimit: 1000 // 调整分块大小警告阈值（KB）
  },
  // 开发服务器配置
  server: {
    // 安全响应头配置
    headers: {
      'Content-Security-Policy': [ // 内容安全策略
        "default-src 'self'", // 默认同源
        "script-src 'self' 'unsafe-inline'", // 脚本源
        "style-src 'self' 'unsafe-inline'", // 样式源
        "img-src 'self' data: blob:", // 图片源
        "font-src 'self'" // 字体源
      ].join('; '),
      'X-Content-Type-Options': 'nosniff', // 禁止MIME嗅探
      'X-Frame-Options': 'DENY', // 禁止嵌入iframe
      'X-XSS-Protection': '1; mode=block', // XSS保护
      'Referrer-Policy': 'strict-origin-when-cross-origin' // 来源策略
    }
  }
})
