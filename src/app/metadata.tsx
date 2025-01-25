import type { Metadata, Viewport } from 'next'

export const metadata: Metadata = {
  title: 'Double-LOVE',
  description: 'Double-LOVE XML 处理工具',
  manifest: '/manifest.json',
  icons: {
    icon: '/favicon.ico',
    apple: [
      { url: '/icon-192x192.png', sizes: '192x192', type: 'image/png' },
      { url: '/icon-512x512.png', sizes: '512x512', type: 'image/png' },
    ],
  },
  appleWebApp: {
    capable: true,
    statusBarStyle: 'default',
    title: 'Double-LOVE',
  },
}

export const viewport: Viewport = {
  themeColor: '#ffffff',
  viewportFit: 'cover',
}
