/** @type {import('tailwindcss').Config} */
export default {
  // 扫描路径配置
  content: [
    "./index.html",          // 主HTML文件
    "./src/**/*.{js,ts,jsx,tsx}", // 所有源代码文件
  ],
  
  // 暗色模式配置（通过class切换）
  darkMode: 'class',
  
  // 主题扩展配置
  theme: {
    extend: {
      // 自定义阴影配置
      boxShadow: {
        'custom': '0 8px 32px rgba(0,0,0,0.12)', // 卡片投影效果
      },
      
      // 颜色系统配置
      colors: {
        selected: '#3366FF',   // 选中状态色
        love: '#EA2AA0',       // 主品牌色
        premiere: '#00005B',   // 深色强调色
        
        // 亮色模式配色
        light: {
          'bg': '#F1F1F1',        // 页面背景色
          'card': '#F9F9F9',      // 卡片背景色
          'input': '#F4F4F4',     // 输入框背景色
          'placeholder': '#0D0D0D', // 占位符文字色
          'titlebar': '#F9F9F9',  // 标题栏背景色
        },
        
        // 暗色模式配色
        dark: {
          'bg': '#212121',        // 页面背景色
          'card': '#171717',      // 卡片背景色
          'input': '#2F2F2F',     // 输入框背景色
          'placeholder': '#ECECEC', // 占位符文字色
          'titlebar': '#171717',   // 标题栏背景色
        }
      },
      
      // 字体配置
      fontFamily: {
        chalkboard: ['"Chalkboard SE"', '"Comic Sans MS"', 'cursive'], // 手写风格字体
      },
    },
  },
  
  // 插件配置（保留空数组表示暂未启用额外插件）
  plugins: [],
}
