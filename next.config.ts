import type { NextConfig } from 'next'
import type { Configuration as WebpackConfig } from 'webpack'

const nextConfig: NextConfig = {
  // 输出独立部署包
  output: 'standalone',
  
  // 生产环境配置
  productionBrowserSourceMaps: false,
  
  // 禁用图片优化，因为 Cloudflare Pages 不支持
  images: {
    unoptimized: true,
  },
  
  // PWA 和安全相关的请求头
  async headers() {
    return [
      {
        // Service Worker 配置
        source: '/sw.js',
        headers: [
          {
            key: 'Cache-Control',
            value: 'public, max-age=0, must-revalidate',
          },
          {
            key: 'Service-Worker-Allowed',
            value: '/',
          },
        ],
      },
      {
        // manifest.json 配置
        source: '/manifest.json',
        headers: [
          {
            key: 'Cache-Control',
            value: 'public, max-age=0, must-revalidate',
          },
        ],
      },
      {
        // 安全相关头部配置
        source: '/:path*',
        headers: [
          {
            key: 'X-Content-Type-Options',
            value: 'nosniff',
          },
          {
            key: 'X-Frame-Options',
            value: 'DENY',
          },
          {
            key: 'X-XSS-Protection',
            value: '1; mode=block',
          },
          {
            key: 'Access-Control-Allow-Credentials',
            value: 'true',
          },
          {
            key: 'Access-Control-Allow-Origin',
            value: '*',
          },
          {
            key: 'Access-Control-Allow-Methods',
            value: 'GET,POST,OPTIONS',
          },
          {
            key: 'Access-Control-Allow-Headers',
            value: 'X-Requested-With,content-type',
          },
        ],
      },
    ]
  },

  // 重写规则
  async rewrites() {
    return {
      beforeFiles: [
        // 确保 Service Worker 可以被正确访问
        {
          source: '/sw.js',
          destination: '/_next/static/sw.js',
        },
      ],
    }
  },

  // 构建时的环境变量
  env: {
    NEXT_PUBLIC_APP_VERSION: process.env.npm_package_version || '1.0.0',
  },
  
  // 编译时优化
  compiler: {
    // 移除 console.log
    removeConsole: process.env.NODE_ENV === 'production',
  },
  
  // 实验性功能
  experimental: {
    // 优化资源加载
    optimizePackageImports: ['evergreen-ui'],
  },
  
  // Webpack 配置
  webpack: (config: WebpackConfig, { isServer }: { isServer: boolean }) => {
    // 优化 XML 相关的处理
    if (!config.module) {
      config.module = { rules: [] }
    }

    if (!config.module.rules) {
      config.module.rules = []
    }

    config.module.rules.push({
      test: /\.xml$/,
      use: 'raw-loader',
    })

    return config
  },
  
  // 设置严格模式
  reactStrictMode: true,
}

export default nextConfig