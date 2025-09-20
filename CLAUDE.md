# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概览

Double Love 是一个专业的场记元数据处理工具，为影视工业化流程提供自动化标准化处理。该项目是一个支持离线使用的渐进式Web应用（PWA），主要用于处理Adobe Premiere Pro XML文件。

## 开发命令

```bash
# 启动开发服务器
pnpm dev

# 构建生产版本（包含TypeScript类型检查）
pnpm build

# 代码质量检查
pnpm lint

# 预览构建结果
pnpm preview
```

**重要说明**: 
- 构建命令会先运行 TypeScript 编译检查，然后使用 Vite 构建
- 没有配置测试框架，当前项目没有测试命令

## 核心架构

### 应用架构模式
- **单页面应用（SPA）**: 基于 React 18 + TypeScript + Vite
- **PWA支持**: 完整的离线功能和Service Worker缓存策略
- **Context模式**: 主题状态管理使用React Context
- **函数式组件**: 全部使用Hooks，无类组件

### 关键技术栈
- **React 18.3.1**: 前端框架
- **TypeScript 5.6.2**: 强类型支持
- **Vite 6.2.1**: 构建工具
- **Tailwind CSS 3.4**: 原子化CSS，支持暗色模式
- **Lucide React**: 图标库

### 核心数据流

1. **文件上传流程**: `DoubleLoveUploader` → 文件验证 → `processXML` → 下载处理结果
2. **XML处理核心**: `src/utils/xml.ts` 包含完整的XML解析、标准化和转换逻辑
3. **主题管理**: `ThemeContext` 自动检测系统主题并管理全局切换

## 代码组织结构

```
src/
├── components/           # React组件
│   ├── DoubleLoveUploader.tsx  # 主要文件处理组件
│   └── PWAUpdatePrompt.tsx     # PWA更新提示
├── context/             # React Context
│   └── ThemeContext.tsx        # 主题状态管理
├── utils/               # 核心工具函数
│   └── xml.ts                  # XML处理逻辑（核心业务）
├── config/              # 配置管理
│   └── version.ts              # 版本信息管理
└── styles/              # 全局样式
    └── index.css               # Tailwind CSS入口
```

## 重要开发规范

### XML处理规范
- XML文件验证: 最大50MB，仅支持XML格式
- 场景号格式: 3位数字 (如: 001)
- 镜头号格式: 2位数字 (如: 01)
- 评分映射: Circle→ok, KEEP→kp, NG→ng
- 自动添加DIT信息: 'DIT: 哆啦Ahua 🌱'

### 文件处理流程
- 支持批量处理（最多99个文件）
- 拖拽上传和点击上传：XML 文件（50MB）+ CSV 文件（10MB）
- 实时进度跟踪
- 自定义前缀和分辨率设置
- **动态Episode命名**: 
  - 有CSV文件时: `{episode}_{scene}_{shot}_{take}{camera}{rating}`
  - 无CSV文件时: `{scene}_{shot}_{take}{camera}{rating}`

### TypeScript接口关键类型
- `XMLProcessConfig`: XML处理配置（包含 csvEpisodeMap 字段）
- `ProcessedClipData`: 处理后的剪辑数据
- `ClipElements`: XML元素集合
- `XMLProcessError`: 自定义错误类型

### 核心函数说明
- `parseCSVForEpisodes()`: 解析CSV文件提取Episode映射数据
- `generateNewName()`: 根据配置生成动态命名格式
- `processXML()`: 主要XML处理函数，支持CSV Episode映射

## PWA配置要点

### Service Worker缓存策略
- 静态资源: 缓存所有 js/css/html/图片 文件
- CDN资源: 1年缓存有效期
- 自动更新: 跳过等待，立即接管控制权

### 图标配置
- 多尺寸图标: 96px, 192px, 512px
- 支持maskable图标（Android适配）
- iOS专用图标: apple-touch-icon.png

## 主题系统

### 实现方式
- 自动检测系统主题偏好
- 通过根元素 `dark` class 切换
- Tailwind CSS暗色模式: `dark:` 前缀

### 颜色系统
```javascript
// 主要品牌色
love: '#EA2AA0'      // 主品牌色
premiere: '#00005B'   // 深色强调色
selected: '#3366FF'   // 选中状态

// 亮色模式配色
light.bg: '#F1F1F1'
light.card: '#F9F9F9'

// 暗色模式配色  
dark.bg: '#212121'
dark.card: '#171717'
```

## 影视行业特定功能

### 支持的工作流
1. DTG Slate → Silverstack Lab → Adobe Premiere Pro XML
2. 场记数据标准化处理
3. 无缝对接Adobe Premiere工作流

### 处理输出示例
- 输入: `UnitA_304_20250127.xml` + `D_0039_1D0U.csv`
- 输出: `UnitA_304_20250127_Double_LOVE.xml`
- 片段命名示例:
  - 有Episode: `002_015_01_01d` (来自CSV的Episode 2)
  - 无Episode: `015_01_01d` (传统格式)

## 安全与性能

### 安全配置
- 内容安全策略（CSP）完整配置
- XSS保护和MIME嗅探防护
- 本地SSL证书支持（开发环境）

### 性能优化
- 代码分割: React库独立分包
- 静态资源压缩和缓存
- PWA离线优先策略
- Source map生成（便于调试）