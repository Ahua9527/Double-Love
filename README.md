# Double-LOVE-Web

Double-LOVE XML 处理工具

## 本地开发

```bash
npm run dev
```

## 部署到Cloudflare Pages

### 准备工作
1. 登录Cloudflare Dashboard
2. 在Pages页面创建新项目
3. 获取API Token和Account ID：
   - 进入My Profile > API Tokens
   - 创建新的API Token，选择"Edit Cloudflare Pages"权限
   - 在Overview页面获取Account ID
4. 在GitHub仓库设置Secrets：
   - 进入Settings > Secrets and variables > Actions
   - 添加以下Secrets：
     - CLOUDFLARE_API_TOKEN
     - CLOUDFLARE_ACCOUNT_ID

### 部署流程
1. 推送代码到main分支
2. 自动触发GitHub Actions部署
3. 在Cloudflare Pages查看部署状态

## 项目结构

- `src/app/` - 主应用代码
- `src/utils/` - 工具函数
- `public/` - 静态资源
