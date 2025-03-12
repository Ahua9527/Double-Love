/**
 * PWA更新提示组件
 * 
 * 主要功能：
 * 1. 监听Service Worker更新事件
 * 2. 显示新版本可用提示
 * 3. 处理用户更新操作
 * 4. 管理提示框的显示状态
 */
import { useEffect, useState } from 'react'
import { X } from 'lucide-react'

/**
 * PWA应用更新提示组件
 * @returns {JSX.Element | null} 更新提示界面或null（当不需要更新时）
 */
const PWAUpdatePrompt = () => {
  // 状态管理是否需要刷新
  const [needRefresh, setNeedRefresh] = useState(false)

  /**
   * 监听PWA更新事件
   * 当检测到新版本Service Worker时触发状态更新
   */
  useEffect(() => {
    // 事件处理器：处理Service Worker更新事件
    const handler = (event: Event) => {
      if ('newServiceWorker' in event && event instanceof CustomEvent) {
        setNeedRefresh(true)
      }
    }

    // 注册自定义事件监听器
    window.addEventListener('pwa-update-available', handler)
    return () => window.removeEventListener('pwa-update-available', handler)
  }, [])

  /**
   * 处理用户确认更新操作
   * 触发更新接受事件并重置状态
   */
  const updateApp = () => {
    const event = new Event('pwa-update-accepted')
    window.dispatchEvent(event)
    setNeedRefresh(false)
  }

  // 不需要更新时返回null
  if (!needRefresh) return null

  return (
    <div 
      // 更新提示框容器样式
      className="fixed bottom-20 left-1/2 transform -translate-x-1/2 bg-white dark:bg-gray-800 
                rounded-lg shadow-lg p-4 flex items-center justify-between gap-4 z-50
                border border-gray-200 dark:border-gray-700 max-w-sm w-11/12"
      role="alert"
      aria-live="polite"
    >
      {/* 提示信息区域 */}
      {/* 提示文本 */}
      <p className="text-sm text-gray-700 dark:text-gray-300">
        新版本可用，是否更新？
      </p>
      
      {/* 按钮操作区域 */}
      <div className="flex items-center gap-2">
        {/* 更新确认按钮 */}
        <button
          onClick={updateApp}
          className="px-3 py-1 bg-selected text-white rounded-md text-sm hover:bg-blue-600 
                     transition-colors duration-200"
          aria-label="确认更新"
        >
          更新
        </button>
        {/* 关闭提示按钮 */}
        <button
          onClick={() => setNeedRefresh(false)}
          className="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-full 
                     transition-colors duration-200"
          aria-label="关闭提示"
        >
          <X className="w-4 h-4 text-gray-500 dark:text-gray-400" />
        </button>
      </div>
    </div>
  )
}

export default PWAUpdatePrompt
