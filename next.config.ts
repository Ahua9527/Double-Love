import { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'export',
  images: {
    unoptimized: true
  },
  trailingSlash: true,
  webpack: (config, { isServer }) => {
    config.cache = false;
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
    return config;
  }
}

export default nextConfig