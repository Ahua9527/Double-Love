'use client'
import { useEffect } from 'react'

export function PWAInitializer() {
  useEffect(() => {
    if (
      typeof window !== 'undefined' &&
      'serviceWorker' in navigator
    ) {
      // 注册 Service Worker
      const registerServiceWorker = async () => {
        try {
          const registration = await navigator.serviceWorker.register('/sw.js');
          console.log('Service Worker registered with scope:', registration.scope);

          // 检查更新
          registration.addEventListener('updatefound', () => {
            const newWorker = registration.installing;
            if (newWorker) {
              newWorker.addEventListener('statechange', () => {
                if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
                  console.log('New content is available; please refresh.');
                }
              });
            }
          });
        } catch (error) {
          console.error('Service Worker registration failed:', error);
        }
      };

      // 在页面加载完成后注册 Service Worker
      if (document.readyState === 'complete') {
        registerServiceWorker();
      } else {
        window.addEventListener('load', registerServiceWorker);
      }
    }
  }, []);

  return null;
}