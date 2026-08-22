# Studio 升级与回退

## 当前升级流程

只有已打包应用会在主窗口加载后静默检查一次更新。当前策略固定为：

- `autoDownload: false`
- `autoInstallOnAppQuit: false`
- `allowPrerelease: false`

GitHub 草稿 Release 对 updater 不可见，公开的 prerelease 也不会进入稳定更新
流。发现正式更新后仍有两次独立确认：第一次确认才下载，下载完成后第二次
确认才退出并安装。普通退出不会顺带安装。

首个 Electron 版本 `0.2.0` 不能由旧 Tauri updater 跨容器安装。旧版用户
必须从正式 Release 手动下载已签名、公证的 DMG，退出旧应用后拖入
Applications，再启动 Electron 版。之后才使用上述 Electron 更新流。

## 首次 Electron 写入前的一次性备份

Electron 第一次可能写入既有数据前会创建一次备份；目标已存在时永不覆盖：

- 应用偏好同目录：`preferences.json.pre-electron-backup`
- 项目数据库目录：`.doublelove/project.pre-electron-backup.sqlite`

偏好备份保留原始字节并使用 `0600`。项目备份通过 SQLite online backup
包含已提交的 WAL 内容。它们是迁移前快照，不是持续备份，也不会随之后的
编辑更新。

### 恢复偏好

1. 完全退出 Electron 与旧应用。
2. 先把当前 `preferences.json` 和一次性备份各复制到另一个安全位置。
3. 复制（不要移动）`preferences.json.pre-electron-backup`，覆盖同目录的
   `preferences.json`。
4. 执行 `chmod 600 preferences.json`，再启动目标旧版本验证。

应用数据目录为：

```text
~/Library/Application Support/space.ahua.doublelove.studio/
```

### 恢复项目数据库

1. 完全退出所有 Double Love 进程，并先复制整个项目的 `.doublelove/`。
2. 把当前 `project.sqlite`、`project.sqlite-wal`、`project.sqlite-shm` 一起
   移出 `.doublelove/`；不要让旧 WAL 套到恢复数据库。
3. 复制（不要移动）`project.pre-electron-backup.sqlite` 为
   `.doublelove/project.sqlite`。
4. 用目标旧版本打开原项目目录，确认 revision、素材与导出记录。

不要直接改写这两个 `.pre-electron-backup` 文件。恢复后的新写入发生在副本。

## 回退锚点

迁移归档位于 [`docs/migration/electron/`](../migration/electron/README.md)。
最后可构建 Tauri 容器的锚点是 `c6d43fb`；在迁移收口提交上可稳定写成
`cf3831a~1` 或 `cf3831a^`，两者都表示删除旧容器之前的父提交。
`cf3831a` 是删除旧容器、进入 Electron-only 工作区的容器迁移提交，不是
Tauri 构建点。

这些 commit 是源码回退参考，不代表已有可下载、已签名的安装包。实际回退
只能使用经过保留和验证的旧版安装包，或在隔离分支按当时工具链重新构建。

## 降级原则

自动 updater 只用于升级，不提供通用降级。需要降级时：

1. 退出应用，复制当前 Application Support 与整个项目目录。
2. 先确认目标旧版能读取当前数据；较新版本写过的数据不能假定向后兼容。
3. 手动安装已签名的目标旧版，不要覆盖唯一一份当前数据。
4. 若明确回到首次 Electron 之前，再按上文使用一次性备份；普通 Electron
   版本间降级应使用该版本升级前另行制作的完整备份。
5. 打开副本验证后，再决定是否把它作为工作副本。

遇到失败先按[排障指南](troubleshooting.md)保留日志，不要反复覆盖数据库。
