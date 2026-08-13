# 更新日志

本项目所有重要变更都将记录于此文件。

格式遵循[Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)规范，
版本号采用[语义化版本](https://semver.org/lang/zh-CN/)标准。

## [未发布]

### 尚未关闭
- 生产部署响应头、VoiceOver 和 Premiere 导入/重连/再导出仍需人工验收；本地真实 Chrome 下载、离线与 PWA 更新流程已通过。
- 独立 `csv2xml` 工作区的 TypeScript 编译链仍由 Format Lab 处理。

### 新增
- 更新日志文件，遵循Keep a Changelog规范
- 引入 Vitest 测试框架，以 62 项测试覆盖 XML/CSV 处理、上传组件、下载名和 PWA 源配置
- PWA 更新提示接入（registerType 改为 prompt，使用 useRegisterSW）
- 为 Cloudflare Pages 静态部署增加受测试保护的 CSP、防嵌入和最小权限响应头

### 变更
- Web 处理结果改为结构化状态、计数和诊断，部分 XML 结果会明确显示为“部分完成”
- CSV 解析支持 BOM、CRLF、引号、字段内逗号和转义引号，并统一忽略路径、常见媒体扩展名和大小写的匹配键
- 上传契约固定为 XML 与 CSV 合计最多 99 个已接受文件，且只允许一个 CSV；重复文件会被忽略
- 集数号格式从3位数字改为2位数字（如：044 → 44）
- 更新相关文档说明和示例
- 评级映射：Circle 显式映射为 ok（原为小写转换输出 circle）
- processXML 接入真实进度回调（onProgress 按已处理 clip 数回调）
- PWA 更新策略从 autoUpdate 改为 prompt 模式（需用户确认后更新）

### 修复
- 移除被 git 跟踪的本地 HTTPS 证书（localhost-key.pem / localhost.pem）
- 下载文件名大小写处理：支持 .XML 大写后缀
- 分辨率输入校验：非正整数时阻止处理并提示
- 更新 Baseline/Browserslist 数据，消除测试和构建中的过期警告
- 移除多文件处理间 1 秒人为延迟
- 移除大量调试 console.log 输出

### 移除
- 无调用方的 parseCSVForEpisodes 死代码
- 无效且未被引用的 public/manifest.json
- 未使用的 Vite 模板残留 App.css 与 css.d.ts
- 未使用的路径别名（@ / @components / @assets）与 cdnjs 运行时缓存配置
- 未启用的 @tailwindcss/forms、@tailwindcss/typography 依赖
- 冗余的 XMLProcessErrorType 枚举值（MISSING_REQUIRED_ELEMENTS / INVALID_FORMAT）

## [0.5.0] - 2025-09-20

### 新增
- Season支持功能，实现完整的动态命名格式系统
- parseCSVForSeasonEpisode函数，支持Season+Episode双重数据解析
- 三种动态命名格式：
  * Season+Episode: `{season}_{episode}_{scene}_{shot}_{take}{camera}_{rating}`
  * Episode only: `E{episode}_{scene}_{shot}_{take}{camera}_{rating}`
  * Traditional: `{scene}_{shot}_{take}{camera}_{rating}`
- 智能显示当前命名模式状态

### 变更
- 扩展XMLProcessConfig接口，增加csvSeasonMap字段支持
- 优化generateNewName函数，支持专业影视制作工作流
- 移除S和E前缀，采用纯数字格式（如01_044_026_04_01d）
- 完善中英文文档，新增详细的命名格式说明和示例

### 移除
- 自定义前缀功能，简化用户界面
- prefix状态和相关UI输入框
- XMLProcessConfig接口中的prefix字段定义

### 修复
- ESLint问题，移除未使用的parseCSVForEpisodes导入

## [0.4.0] - 2025-09-20

### 新增
- CSV文件支持和Episode命名功能
- CSV文件上传和解析功能，支持Episode映射数据
- parseCSVForEpisodes函数解析Name到Episode映射
- 动态命名格式，有CSV时使用{episode}_{scene}_{shot}格式

### 变更
- 优化文件处理流程，分离XML和CSV文件管理
- 改进processXML函数支持csvEpisodeMap参数
- 增强UI显示CSV文件状态和Episode使用提示
- 从npm迁移到pnpm包管理器

### 移除
- package-lock.json文件
- npm相关配置和文档

## [0.3.0] - 2025-09-20

### 新增
- pathURL处理功能，支持序列帧文件名规范化
- 版本号显示功能
- DIT信息生成格式为网站链接

### 变更
- 重新设计版本号显示格式
- 优化文件上传区域的拖拽交互动画和视觉反馈
- 改进文件列表和处理按钮的UI细节
- 完善项目配置文件忽略规则

## [0.2.0] - 2025-04-01

### 新增
- 前端页面版本号显示
- `<clip>`元素中的`<labels>`复制到`<sequence>`功能

### 变更
- 处理Label元素，修正来自Pomfort的Cerulean拼写错误
- 将clip的labels元素复制到sequence元素

### 修复
- 从Label中提取拍摄评级的规则优化：
  * 如果包含"No Label"则不提取（返回空字符串）
  * 特殊处理: "keep"或"kp"统一返回"kp"
  * 其他任何内容都提取并转换为小写

### 移除
- 从tags中提取评级的功能

## [0.1.0] - 2025-01-31 ~ 2025-03-13

### 新增
- 项目初始化和核心功能实现
- Adobe Premiere Pro XML文件处理功能
- PWA支持，完整的离线功能
- 暗色模式主题系统
- HTTPS支持和localhost证书
- MIT开源许可证
- 项目README文档

### 变更
- 重写所有代码注释
- 修正网站标题颜色
- 添加项目Slogan

### 移除
- .idea IDE配置文件
- 无效的favicon文件

### 修复
- favicon显示问题
- 网站标题颜色显示
