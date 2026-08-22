// Studio 工作区独立 ESLint 配置（与根项目同款规则）
import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'

const electronFiles = [
  'src/main/**/*.ts',
  'src/preload/**/*.ts',
  'electron.vite.config.ts',
  'playwright.config.ts',
  'e2e/**/*.ts',
]

export default tseslint.config(
  { ignores: ['dist', 'out'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
    },
  },
  {
    files: ['src/renderer/**/*.{ts,tsx}'],
    languageOptions: {
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],
    },
  },
  {
    files: electronFiles,
    languageOptions: {
      globals: globals.node,
    },
  },
)
