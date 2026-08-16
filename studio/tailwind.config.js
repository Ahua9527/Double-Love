/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],

  // 暗色模式通过根元素 dark class 切换（跟随系统）
  darkMode: 'class',

  theme: {
    extend: {
      colors: {
        // 品牌色：仅品牌标识与关键操作
        love: '#EA2AA0',
        premiere: '#00005B',
        selected: '#3366FF',
        playhead: '#E5484D',
        trackb: '#12A594',
        audiogreen: '#30A46C',
        csvpurple: '#8E4EC6',
        // 状态色
        success: '#30A46C',
        warning: '#D99A00',
        danger: '#E5484D',
        info: '#0090FF',
        // 主题化表面色（随 dark class 切换，定义见 index.css）
        surface: 'var(--surface)',
        card: 'var(--card)',
        fg: 'var(--fg)',
        mutedfg: 'var(--muted-fg)',
        line: 'var(--line)',
        sidebar: 'var(--sidebar)',
        sidebarline: 'var(--sidebar-line)',
        sidebaraccent: 'var(--sidebar-accent)',
        table: 'var(--table)',
        tablehead: 'var(--table-head)',
        tableheadfg: 'var(--table-head-fg)',
        tableline: 'var(--table-line)',
        tablehover: 'var(--table-hover)',
      },
      fontFamily: {
        // macOS 系统字体栈
        sans: ['-apple-system', 'BlinkMacSystemFont', '"SF Pro Text"', '"Helvetica Neue"', 'Arial', 'sans-serif'],
        mono: ['ui-monospace', '"SF Mono"', 'Menlo', 'Consolas', 'monospace'],
      },
    },
  },

  plugins: [],
}
