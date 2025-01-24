  // app/pwa.tsx
  'use client'
  import { useEffect } from 'react'
  
  export function PWAInitializer() {
    useEffect(() => {
      if ('serviceWorker' in navigator && window.location.hostname !== 'localhost') {
        navigator.serviceWorker
          .register('/sw.js')
          .then((registration) => {
            console.log('Service Worker registered:', registration);
          })
          .catch((error) => {
            console.error('Service Worker registration failed:', error);
          });
      }
    }, []);
  
    return null;
  }