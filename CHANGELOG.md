# 更新日志

本项目所有重要变更都将记录于此文件。

格式遵循[Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)规范，
版本号采用[语义化版本](https://semver.org/lang/zh-CN/)标准。

## [未发布]

### 新增
- 更新日志文件，遵循Keep a Changelog规范

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