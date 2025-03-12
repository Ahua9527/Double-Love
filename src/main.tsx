// React核心库导入
import React from 'react'
// React DOM客户端渲染库
import ReactDOM from 'react-dom/client'
// 主应用组件
import App from './App.tsx'
// 全局样式文件
import './styles/index.css'

// 创建React根节点并挂载应用
ReactDOM.createRoot(
  // 使用非空断言操作符(!)确保root元素存在
  document.getElementById('root')!
).render(
  // 启用严格模式（开发环境额外检查）
  <React.StrictMode>
    {/* 应用主组件 */}
    <App />
  </React.StrictMode>,
)

// 页面加载完成后添加样式类（可选操作）
// 使用可选链操作符(?.)安全访问DOM元素
document.getElementById('root')?.classList.add('loaded')
