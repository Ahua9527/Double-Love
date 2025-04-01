// vite.config.ts

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'
import fs from 'fs'
// 移除未使用的path导入
// import path from 'path'

// 读取package.json获取版本号
const packageJson = JSON.parse(fs.readFileSync('./package.json', 'utf-8'));
const { version } = packageJson;

// 主配置对象
export default defineConfig({
  // 插件配置
  plugins: [
    react(), // 启用React支持
    
    // PWA配置
    VitePWA({
      registerType: 'autoUpdate', // 自动更新策略
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
        skipWaiting: true, // 跳过等待阶段
        clientsClaim: true, // 立即接管控制权
        
        // 缓存匹配模式
        globPatterns: [
          '**/*.{js,css,html,ico,png,svg,woff2,jpg,jpeg,gif,json,webp}' // 缓存所有静态资源
        ],
        
        // 运行时缓存策略
        runtimeCaching: [
          {
            // CDN资源缓存策略
            urlPattern: /^https:\/\/cdnjs\.cloudflare\.com\/.*/i, // 匹配CDN地址
            handler: 'CacheFirst', // 优先使用缓存
            options: {
              cacheName: 'cdn-cache', // 缓存名称
              expiration: {
                maxEntries: 20, // 最大条目数
                maxAgeSeconds: 60 * 60 * 24 * 365 // 缓存有效期（1年）
              },
              cacheableResponse: {
                statuses: [0, 200] // 缓存响应状态码（0表示离线）
              }
            }
          }
        ]
      }
    })
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
  // 模块解析配置
  resolve: {
    alias: {
      '@': '/src', // 根路径别名
      '@components': '/src/components', // 组件目录别名
      '@assets': '/src/assets' // 资源目录别名
    }
  },
  // 开发服务器配置
  server: {
    // HTTPS配置
    https: {
      key: fs.readFileSync('localhost-key.pem'), // SSL私钥
      cert: fs.readFileSync('localhost.pem'), // SSL证书
    },
    
    // 安全响应头配置
    headers: {
      'Content-Security-Policy': [ // 内容安全策略
        "default-src 'self'", // 默认同源
        "script-src 'self' 'unsafe-inline' cdnjs.cloudflare.com", // 脚本源
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