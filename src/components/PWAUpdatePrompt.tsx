/**
 * PWA更新提示组件
 * 
 * 主要功能：
 * 1. 通过 useRegisterSW 注册 Service Worker（registerType: 'prompt'）
 * 2. 检测到新版本时显示更新提示
 * 3. 处理用户更新操作
 * 4. 首次可离线时提示一次性 toast
 */
import { useEffect, useState } from 'react'
import { X } from 'lucide-react'
import { useRegisterSW } from 'virtual:pwa-register/react'

/**
 * PWA应用更新提示组件
 * @returns {JSX.Element | null} 更新提示界面或null（当不需要更新时）
 */
const PWAUpdatePrompt = () => {
  // useRegisterSW：注册 SW 并在有新版本（needRefresh）或可离线（offlineReady）时置位
  const {
    offlineReady: [offlineReady, setOfflineReady],
    needRefresh: [needRefresh, setNeedRefresh],
    updateServiceWorker,
  } = useRegisterSW()

  // 首次可离线时显示一次性提示
  const [showOfflineTip, setShowOfflineTip] = useState(false)
  useEffect(() => {
    if (offlineReady) {
      setShowOfflineTip(true)
      setOfflineReady(false)
    }
  }, [offlineReady, setOfflineReady])

  // 无更新且无离线提示时返回null
  if (!needRefresh && !showOfflineTip) return null

  const close = () => {
    setNeedRefresh(false)
    setShowOfflineTip(false)
  }

  return (
    <div
      className="fixed bottom-20 left-1/2 transform -translate-x-1/2 bg-white dark:bg-gray-800 
                rounded-lg shadow-lg p-4 flex items-center justify-between gap-4 z-50
                border border-gray-200 dark:border-gray-700 max-w-sm w-11/12"
      role="alert"
      aria-live="polite"
    >
      {/* 提示信息区域 */}
      <p className="text-sm text-gray-700 dark:text-gray-300">
        {needRefresh ? '新版本可用，是否更新？' : '已可离线使用'}
      </p>

      {/* 按钮操作区域 */}
      <div className="flex items-center gap-2">
        {needRefresh && (
          <button
            onClick={() => updateServiceWorker(true)}
            className="px-3 py-1 bg-selected text-white rounded-md text-sm hover:bg-blue-600
                       transition-colors duration-200"
            aria-label="确认更新"
          >
            更新
          </button>
        )}
        {/* 关闭提示按钮 */}
        <button
          onClick={close}
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
