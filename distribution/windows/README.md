# Windows 本机启动与更新

本机 2026-09-09 已发布至 `%LOCALAPPDATA%\agent-steward-app-schema5`，使用桌面 **Agent Steward** 入口或该目录的 `Start.cmd`。后续源码更新请显式传入这个 `-InstallRoot`，不要误用旧默认安装目录：

```powershell
.\distribution\windows\update-local.ps1 -InstallRoot "$env:LOCALAPPDATA\agent-steward-app-schema5" -NoOpen
```

下方默认目录示例适用于新安装；自定义安装始终沿用自身根目录。

## 推荐：当前源码本地编译更新

在仓库中双击 **`distribution\windows\Update-Local.cmd`**。

- 使用当前工作树（包括未提交改动），不执行 Git pull、checkout、reset 或 stash。
- 需要 Rust/Cargo 和 Windows MSVC C++ 构建工具；首次编译较慢，后续复用独立编译缓存。Cargo 可能下载缺失依赖，但不会下载 GitHub 发布包。
- 首次填写监听 IP（例如 `172.19.10.185`）、端口、数据库和 runtime 路径。已有服务用过自定义路径时务必沿用，避免误以为原任务丢失。
- 后续直接双击：先编译并安装到新的版本目录，再正常停止启动器管理的旧服务、启动新版并检查监听端口归属及 HTTP 响应。
- 编译、包兼容或数据库版本预检失败不会停止旧服务；本地更新也不会在预检失败时覆盖稳定启动器。新版启动失败只尝试通过再次预检的同 Schema 原版本；停止超时不强杀，不并行启动第二个实例。
- 当前包及启动器只接受 **Schema6**。数据库和凭据不随程序替换，不自动迁移或恢复备份；已有 Schema 2/4/5 安装不走普通 Update/Update-Local，须先单独安排停写、最终一致备份及显式迁移，再使用独立安装目录，保留旧安装用于受控恢复。不会将旧程序自动回退到已经迁移的库上。

首次接入启动器前，请自行在旧服务终端按 Ctrl+C 停止它。启动器**不接管、不强杀手工启动的进程**，也不替换 PATH 中的 taskctl/task-hook；宿主 Hook 如需使用新 CLI，应另行更新其可执行文件路径。

也可以用 PowerShell：

```powershell
cd E:\ai\agent-steward
.\distribution\windows\update-local.ps1
# 指定隔离安装位置，不自动打开浏览器：
.\distribution\windows\update-local.ps1 -InstallRoot E:\steward-app -NoOpen
```

不要把安装位置设为源码或数据库目录。编译期间不要修改源码，以免构建混合版本。

### 从 Schema 2/4/5 进入 Schema6

这是单独授权的维护操作，不是普通更新。按 [Schema6迁移与待上线合同](../../docs/v0/21-待上线任务状态.md) 完成停写、备份、迁移及核验后，选择**新的独立 InstallRoot**，明确配置已核验的 Schema6 数据库和原 runtime 路径。数据库可以在获准后保留原正式路径，但程序安装目录不复用旧 Schema 2/4/5 的 current/previous 指针。不要复制旧 current.json、previous.json 或 process.json 到新安装。停旧服务使用原安装目录的 Stop.cmd 并核对进程身份，新启动器不接管旧 Schema 安装。

安装准备和调用示意（不代表已获得正式操作授权）：

```powershell
# 初次设置必须明确选择已迁移并核验的库；源库未迁移时会拒绝启动。
pwsh -NoProfile -File .\distribution\windows\update-local.ps1 -InstallRoot E:\steward-schema5-app -NoOpen
# 后续源码更新仍使用这个显式 InstallRoot，不要误指向旧默认安装。
```

新目录内复制的 Start.cmd / Stop.cmd / Update.cmd 根据旁边的 current.json 定位自身安装，不会回到旧默认目录；显式 -InstallRoot 始终优先。旧入口、PATH 中的 taskctl 和各宿主 Hook 路径仍须在维护方案中逐项切换，脚本不会隐式修改它们，也不会自行恢复业务写入。

### 启动前版本预检

启动器在停止旧进程之前调用目标包的：

```text
taskd --check-database-schema --database <明确的本地绝对路径>
```

成功只输出 `databaseSchema=6`，随后退出；不创建数据库、父目录、runtime、凭据或监听器。不存在且无 sidecar 的路径允许之后正常首次初始化；已有库必须是普通文件且版本为6，Schema 0/1/2/3/4/5/未知版本、无法解读的 SQLite 文件、歧义路径或非法 sidecar 均拒绝。读取 SQLite 已提交 WAL 视图，不从可能过期的主文件头猜测版本。

这是**只读版本预检，不是完整性/业务验收、原子切换或停写锁**。WAL 只读访问仍可能涉及 SHM。需要可信、受控路径和维护窗口；预检后数据库变化会被后续启动/回退检查再次拒绝，但不能替代暂停所有写入者。

## 纯 UI 独立更新（不重启 taskd）

前端源码在 `web/`；先用 Node24.11.1 执行 `npm ci --ignore-scripts` 和 `npm run build`。`ui.ps1 Build` 只读取 `web/dist` 三文件，不再发布旧 `crates/server/web`。后端发布前另执行 `npm run sync:embedded`，将相同只读产物同步到 `crates/server/web-readonly` 后重编译 taskd；这样损坏 UI 包或显式回退也不会恢复旧业务写入口。普通 Web build 不修改内嵌快照，Rust 单独构建使用已保存快照。

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

脚本支持 Windows PowerShell 5.1 和 PowerShell 7，JSON 明确按 UTF-8 读取，保留中文路径。本机验证中，5.1 继承其它宿主的 PSModulePath 时 Get-FileHash/Get-Acl 不可用；使用 5.1 自身默认模块路径的隔离子进程后可用，PowerShell 7 也通过验证。请使用适合该宿主的执行环境，脚本不修改全机环境、模块搜索路径或执行策略。如果组织策略禁止脚本，请使用经管理员批准的运行方式，不绕过企业策略。

## 可选：发布包更新

维护者发布稳定版本后，可从 GitHub Releases 下载 `agent-steward-windows-x64.zip` 及 `.sha256`，核对 SHA-256，再解压并运行包里的 `Start.cmd`；本机不需要 Rust。

已安装官方版本时，`Update.cmd` 从固定仓库 `cecil-su/agent-steward` 获取最新稳定发布包，下载和校验完成后才切换服务。校验和是传输完整性检查，不是独立代码签名。本地构建请继续使用仓库内的 `Update-Local.cmd`，不自动与发布版本比较或降级。

发布工作流仅在维护者明确推送 `vMAJOR.MINOR.PATCH` 标签后触发；编写工作流不代表已经发布。更新器不会推送代码或创建 Release。
