import React, { createContext, useContext, useEffect, useState } from 'react';

/**
 * 主题类型定义
 * @typedef {'light' | 'dark'} Theme
 * 表示应用支持的两种主题模式：明亮(light)和暗黑(dark)
 */

/**
 * 主题上下文类型定义
 * @typedef {Object} ThemeContext
 * @property {Theme} theme - 当前应用主题
 */
type Theme = 'light' | 'dark';
type ThemeContext = { theme: Theme };

/**
 * 创建主题上下文
 * @type {React.Context<ThemeContext | undefined>}
 * 初始化时设置为undefined，确保在Provider外使用时会抛出错误
 */
const ThemeContext = createContext<ThemeContext | undefined>(undefined);

/**
 * 主题提供者组件
 * @param {Object} props - 组件属性
 * @param {React.ReactNode} props.children - 子组件
 * @returns {JSX.Element} 主题上下文提供者
 * 
 * 主要功能：
 * 1. 管理主题状态
 * 2. 自动响应系统主题变化
 * 3. 同步主题状态到DOM class
 */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  // 主题状态管理，默认使用light主题
  const [theme, setTheme] = useState<Theme>('light');

  /**
   * 主题效果处理
   * 监听系统主题变化并自动更新应用主题
   */
  useEffect(() => {
    // 创建媒体查询监听器，匹配暗色主题偏好
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');

    /**
     * 主题更新处理函数
     * @param {MediaQueryListEvent | MediaQueryList} e - 媒体查询事件或对象
     */
    const updateTheme = (e: MediaQueryListEvent | MediaQueryList) => {
      // 根据媒体查询结果切换主题
      const newTheme = e.matches ? 'dark' : 'light';
      setTheme(newTheme);
      updateDocumentClass(newTheme);
    };

    // 初始设置主题
    updateTheme(mediaQuery);

    // 注册主题变化监听器
    mediaQuery.addEventListener('change', updateTheme);
    
    // 清理函数：移除事件监听
    return () => mediaQuery.removeEventListener('change', updateTheme);
  }, []);

  /**
   * 更新DOM class的方法
   * @param {Theme} newTheme - 新主题
   * 通过修改根元素的class实现全局样式切换
   */
  const updateDocumentClass = (newTheme: Theme) => {
    // 根据主题切换dark类名
    if (newTheme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  };

  // 提供主题上下文给子组件
  return (
    <ThemeContext.Provider value={{ theme }}>
      {children}
    </ThemeContext.Provider>
  );
}

/**
 * 自定义主题Hook
 * @returns {ThemeContext} 当前主题上下文
 * @throws {Error} 当在ThemeProvider外使用时抛出错误
 * 
 * 使用示例：
 * const { theme } = useTheme();
 */
export function useTheme() {
  const context = useContext(ThemeContext);
  if (context === undefined) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}
