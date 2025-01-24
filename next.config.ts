// next.config.ts
import { NextConfig } from 'next'

const nextConfig: NextConfig = {
 output: 'static',
 trailingSlash: true,
 images: {
   unoptimized: true
 },
 assetPrefix: '.',
 basePath: ''
}

export default nextConfig