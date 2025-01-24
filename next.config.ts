import { NextConfig } from 'next'

// const nextConfig: NextConfig = {
//   output: 'export',
//   reactStrictMode: true,
//   images: {
//     unoptimized: true
//   },
//   trailingSlash: true,
  assetPrefix: process.env.NODE_ENV === 'production' ? 'https://f6291174.double-love-web.pages.dev' : undefined,
  webpack: (config, { isServer }) => {
    config.cache = false
    if (!isServer) {
      config.optimization = {
        ...config.optimization,
        minimize: true,
        splitChunks: {
          chunks: 'all',
          maxSize: 200000,
          minSize: 10000
        }
      }
    }
    return config
  }
}

export default nextConfig