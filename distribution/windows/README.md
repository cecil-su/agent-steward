# Windows 本机启动与更新

## 推荐：当前源码本地编译更新

在仓库中双击 **`distribution\windows\Update-Local.cmd`**。

- 使用当前工作树（包括未提交改动），不执行 Git pull、checkout、reset 或 stash。
- 需要 Rust/Cargo 和 Windows MSVC C++ 构建工具；首次编译较慢，后续复用独立编译缓存。Cargo 可能下载缺失依赖，但不会下载 GitHub 发布包。
- 首次填写监听 IP（例如 `172.19.10.185`）、端口、数据库和 runtime 路径。已有服务用过自定义路径时务必沿用，避免误以为原任务丢失。
- 后续直接双击：先编译并安装到新的版本目录，再正常停止启动器管理的旧服务、启动新版并检查监听端口归属及 HTTP 响应。
- 编译失败不会停止旧服务。新版启动失败尝试重启原版本；停止超时不强杀，不并行启动第二个实例。
- 数据库和凭据不随程序替换。不自动迁移数据库，不自动恢复数据备份。仅支持当前 schema 2；未来格式变化必须单独备份并安排维护。

首次接入启动器前，请自行在旧服务终端按 Ctrl+C 停止它。启动器**不接管、不强杀手工启动的进程**，也不替换 PATH 中的 taskctl/task-hook；宿主 Hook 如需使用新 CLI，应另行更新其可执行文件路径。

也可以用 PowerShell：

```powershell
cd E:\ai\agent-steward
.\distribution\windows\update-local.ps1
# 指定隔离安装位置，不自动打开浏览器：
.\distribution\windows\update-local.ps1 -InstallRoot E:\steward-app -NoOpen
```

不要把安装位置设为源码或数据库目录。编译期间不要修改源码，以免构建混合版本。

## 纯 UI 独立更新（不重启 taskd）

首次需经用户授权升级一次 taskd；新启动器为声明 `uiPackageProtocol: 1` 的二进制包传入 `<InstallRoot>/ui`。之后使用源码内 `ui.ps1`，不再为纯 UI 改动运行 `Update-Local.cmd`：

```powershell
.\distribution\windows\ui.ps1 -Action Build -Version ui-20260907-1 -Output E:\steward-artifacts\ui-20260907-1
.\distribution\windows\ui.ps1 -Action Activate -Package E:\steward-artifacts\ui-20260907-1
.\distribution\windows\ui.ps1 -Action Status
.\distribution\windows\ui.ps1 -Action Rollback
```

支持 `-InstallRoot`；手工启动时同时指定 `-UiRoot` 和目标 taskd 的 `-Url`。包和活动指针整体切换，旧包不自动清理；页面空闲时自动刷新，编辑或提交时提示确认，不打断输入。仅安装可信 UI 包：哈希不替代发布签名。该脚本不下载、不停启服务、不替换后端或数据。完整包合同、回退、兼容边界及隔离测试见 [UI 独立发布](../../docs/v0/14-UI独立发布.md)。

## 日常启动与配置

默认程序安装在 `%LOCALAPPDATA%\agent-steward-app`：

- `Start.cmd`：启动已安装版本；已运行时只打开页面，不重新编译。
- `Stop.cmd`：请求正常退出，最长等待 60 秒，不强杀。
- `settings.json`：持久保存 IP、端口、数据库路径、runtime 路径和 `requireLocalAuth`。更新不会覆盖。
- `versions/`：保留各版本程序；`current.json`、`previous.json` 指向当前/前一版本。
- `build-cache/`：本地编译缓存；`runs/`：启动日志和停止标记。

默认任务数据仍是 `%LOCALAPPDATA%\agent-steward\steward.db`，与程序目录分开。

本机直接访问回环地址或配置的本机网卡 IP 时免登录；其他设备首次使用只读凭据。此模式信任本机所有用户/程序，**禁止通过本机代理、隧道或端口转发暴露服务**。需要严格认证时，设置 `requireLocalAuth: true`；该模式浏览器首次需凭据，Cookie 保留 30 天。局域网 HTTP 不加密，也不会自动配置防火墙或开机自启。

脚本不修改 PowerShell 执行策略。如果组织策略禁止脚本，请使用经管理员批准的运行方式，不绕过企业策略。

## 可选：发布包更新

维护者发布稳定版本后，可从 GitHub Releases 下载 `agent-steward-windows-x64.zip` 及 `.sha256`，核对 SHA-256，再解压并运行包里的 `Start.cmd`；本机不需要 Rust。

已安装官方版本时，`Update.cmd` 从固定仓库 `cecil-su/agent-steward` 获取最新稳定发布包，下载和校验完成后才切换服务。校验和是传输完整性检查，不是独立代码签名。本地构建请继续使用仓库内的 `Update-Local.cmd`，不自动与发布版本比较或降级。

发布工作流仅在维护者明确推送 `vMAJOR.MINOR.PATCH` 标签后触发；编写工作流不代表已经发布。更新器不会推送代码或创建 Release。
