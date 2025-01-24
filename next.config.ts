// next.config.ts
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
        source: '/sw.js',
        headers: [
          {
            key: 'Cache-Control',
            value: 'no-store, no-cache, must-revalidate',
          },
          {
            key: 'Content-Type',
            value: 'application/javascript; charset=utf-8',
          }
        ],
      },
      {
        source: '/manifest.json',
        headers: [
          {
            key: 'Cache-Control',
            value: 'no-store, no-cache, must-revalidate',
          }
        ],
      }
    ]
  },

  // ESLint 配置
  eslint: {
    ignoreDuringBuilds: true,
  },
  
  // Webpack 配置
  webpack: (config: WebpackConfig, { isServer }: { isServer: boolean }) => {
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