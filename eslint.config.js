// ESLint核心配置
import js from '@eslint/js'
// 全局变量定义（浏览器环境）
import globals from 'globals'
// React Hooks规则插件
import reactHooks from 'eslint-plugin-react-hooks'
// React Fast Refresh插件
import reactRefresh from 'eslint-plugin-react-refresh'
// TypeScript ESLint配置
import tseslint from 'typescript-eslint'

export default tseslint.config(
  // 基础配置：忽略dist目录
  { ignores: ['dist'] },
  // 主配置对象
  {
    // 继承的规则集：ESLint推荐 + TypeScript推荐
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    // 应用范围：所有TS/TSX文件
    files: ['**/*.{ts,tsx}'],
    // 语言环境配置
    languageOptions: {
      ecmaVersion: 2020,    // ECMAScript 2020标准
      globals: globals.browser, // 浏览器全局变量（如window/document等）
    },
    // 启用的插件
    plugins: {
      'react-hooks': reactHooks, // React Hooks规则
      'react-refresh': reactRefresh, // React热重载规则
    },
    // 自定义规则配置
    rules: {
      ...reactHooks.configs.recommended.rules, // Hooks规则（包括依赖项检查）
      // React Refresh规则：只允许导出组件（允许常量导出）
      'react-refresh/only-export-components': [
        'warn', // 警告级别
        { allowConstantExport: true }, // 允许导出常量
      ],
    },
  },
)
