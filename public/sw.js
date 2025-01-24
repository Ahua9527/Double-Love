// public/sw.js
const CACHE_NAME = 'double-love-cache-v1';

// 需要缓存的核心资源
const CORE_ASSETS = [
  '/',
  '/manifest.json'
];

// 安装事件
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME)
      .then(async (cache) => {
        // 尝试缓存核心资源，忽略失败的请求
        const cachePromises = CORE_ASSETS.map(url => {
          return cache.add(url).catch(error => {
            console.warn(`Failed to cache: ${url}`, error);
            return Promise.resolve(); // 继续处理其他资源
          });
        });
        
        return Promise.all(cachePromises);
      })
      .then(() => {
        console.log('Core assets cached successfully');
      })
  );
  // 立即激活
  self.skipWaiting();
});

// 激活事件
self.addEventListener('activate', (event) => {
  event.waitUntil(
    Promise.all([
      // 清理旧缓存
      caches.keys().then(cacheNames => {
        return Promise.all(
          cacheNames.map(cacheName => {
            if (cacheName !== CACHE_NAME) {
              return caches.delete(cacheName);
            }
          })
        );
      }),
      // 立即获得控制权
      clients.claim()
    ])
  );
});

// 请求拦截
self.addEventListener('fetch', (event) => {
  // 只处理 GET 请求
  if (event.request.method !== 'GET') return;

  // 排除一些不需要缓存的请求
  if (event.request.url.includes('/api/') || 
      event.request.url.includes('/analytics/') ||
      event.request.url.includes('chrome-extension://')) {
    return;
  }

  event.respondWith(
    caches.match(event.request)
      .then(response => {
        if (response) {
          // 返回缓存的响应
          return response;
        }

        // 如果没有缓存，发起网络请求
        return fetch(event.request)
          .then(response => {
            // 检查响应是否有效
            if (!response || response.status !== 200) {
              return response;
            }

            // 克隆响应
            const responseToCache = response.clone();

            // 尝试缓存新的响应
            caches.open(CACHE_NAME)
              .then(cache => {
                cache.put(event.request, responseToCache)
                  .catch(error => {
                    console.warn('Failed to cache:', event.request.url, error);
                  });
              });

            return response;
          })
          .catch(error => {
            console.error('Fetch failed:', error);
            // 这里可以返回一个离线页面或错误页面
            return new Response('Offline');
          });
      })
  );
});

// 错误处理
self.addEventListener('error', (event) => {
  console.error('Service Worker error:', event.error);
});

self.addEventListener('unhandledrejection', (event) => {
  console.error('Unhandled rejection in Service Worker:', event.reason);
});