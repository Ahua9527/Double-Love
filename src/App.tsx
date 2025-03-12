/**
 * 应用根组件
 * 
 * 主要功能：
 * 1. 提供主题上下文（暗黑/明亮模式）
 * 2. 渲染核心上传组件
 * 3. 管理PWA更新提示
 */
import DoubleLoveUploader from './components/DoubleLoveUploader' // 正确路径导入上传组件
import PWAUpdatePrompt from './components/PWAUpdatePrompt'
import { ThemeProvider } from './context/ThemeContext'

/**
 * 应用根组件
 * @returns {JSX.Element} 返回应用根组件结构
 */
function App(): JSX.Element {
  return (
    // 使用ThemeProvider包裹应用组件
    <ThemeProvider>
      {/* 主容器：设置最小高度和背景色过渡效果 */}
      <div className="min-h-screen bg-gray-50 dark:bg-gray-900 transition-colors duration-300">
        
        {/* 文件上传核心组件 */}
        <DoubleLoveUploader />

        {/* PWA更新提示组件 */}
        <PWAUpdatePrompt />
      </div>
    </ThemeProvider>
  )
}

export default App
