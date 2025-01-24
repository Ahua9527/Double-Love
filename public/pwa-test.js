// public/pwa-test.js
window.testPWA = async function testPWA() {
  console.group('PWA 测试报告');
  console.log('开始测试...\n');

  // 1. Service Worker 测试
  console.group('1. Service Worker 状态');
  try {
    const registration = await navigator.serviceWorker.getRegistration();
    if (registration) {
      console.log('✅ Service Worker 已注册');
      console.table({
        scope: registration.scope,
        状态: registration.active ? '活跃' : '未激活',
        更新状态: registration.waiting ? '有更新等待' : '无更新'
      });
    } else {
      console.warn('⚠️ Service Worker 未注册');
    }
  } catch (err) {
    console.error('❌ Service Worker 检查失败:', err);
  }
  console.groupEnd();

  // 2. Web Manifest 测试
  console.group('2. Web Manifest 状态');
  const manifestLink = document.querySelector('link[rel="manifest"]');
  if (manifestLink) {
    try {
      const response = await fetch(manifestLink.href);
      const manifest = await response.json();
      console.log('✅ Manifest 文件已找到');
      console.table({
        名称: manifest.name,
        短名称: manifest.short_name,
        启动URL: manifest.start_url,
        显示模式: manifest.display,
        主题色: manifest.theme_color,
        背景色: manifest.background_color
      });
    } catch (err) {
      console.error('❌ Manifest 解析失败:', err);
    }
  } else {
    console.warn('⚠️ 未找到 Manifest 文件');
  }
  console.groupEnd();

  // 3. 缓存测试
  console.group('3. 缓存存储状态');
  try {
    const caches = await window.caches.keys();
    if (caches.length > 0) {
      console.log('✅ 已发现缓存');
      for (const cacheName of caches) {
        const cache = await window.caches.open(cacheName);
        const keys = await cache.keys();
        console.log(`${cacheName}: ${keys.length} 个资源`);
        console.table(keys.map(key => ({
          URL: key.url,
          方法: key.method
        })));
      }
    } else {
      console.warn('⚠️ 未发现缓存存储');
    }
  } catch (err) {
    console.error('❌ 缓存检查失败:', err);
  }
  console.groupEnd();

  // 4. 安装状态检查
  console.group('4. PWA 安装状态');
  const isStandalone = window.matchMedia('(display-mode: standalone)').matches ||
                      window.navigator.standalone ||
                      document.referrer.includes('android-app://');
  
  if (isStandalone) {
    console.log('✅ 已作为独立应用安装');
  } else {
    console.log('ℹ️ 可以安装为独立应用');
  }
  console.groupEnd();

  // 5. 网络状态
  console.group('5. 网络连接状态');
  console.log(`当前状态: ${navigator.onLine ? '在线' : '离线'}`);
  if (navigator.onLine) {
    console.log('ℹ️ 提示：可以在开发者工具中切换到离线模式测试离线功能');
  }
  console.groupEnd();

  console.groupEnd();
};

// 添加网络状态监听
window.addEventListener('online', () => console.log('🌐 网络已连接'));
window.addEventListener('offline', () => console.log('⚡ 网络已断开'));

// 初始化提示
console.log('%c PWA 测试工具已加载 ', 'background: #4CAF50; color: white; padding: 2px 5px; border-radius: 3px;');
console.log('输入 await testPWA() 开始测试');