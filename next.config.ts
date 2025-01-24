import { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'export',
  images: {
    unoptimized: true
  },
  assetPrefix: '/',
  basePath: ''
}

export default nextConfig