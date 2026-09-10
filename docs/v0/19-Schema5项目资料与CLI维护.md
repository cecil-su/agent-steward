# Schema5 项目资料与 CLI 维护

## 状态与边界

本轮新增独立项目资料，复用已有 Project 身份、revision CAS、History 和任务归属。Web 仍只读；资料写入仅由 CLI/application 提供，没有新增 HTTP 业务写入口。

**2026-09-09，经用户明确授权完成本机正式发布。** 版本 `local-20260909070803-dcbe72ce96f34cdcb481f200d067d554`，安装根 `%LOCALAPPDATA%/agent-steward-app-schema5`，地址仍为 `http://172.19.10.185:51850`。PATH 三件套和桌面 `Agent Steward` 入口已更新，正式 DB/runtime 路径不变。未创建 GitHub Release、未提交或推送代码。

当前程序及 Windows 打包/启动器只接受 Schema5；新增包构建前版本探测，拒绝把旧二进制标成5。`ui.ps1 Build` 使用 `web/dist`，显式 `npm run sync:embedded` 将相同只读三文件同步到 `crates/server/web-readonly`，重新编译后的内嵌回退和旧资源别名也只读。Schema4 仍不会被普通更新隐式升级。

本轮没有向正式项目填写资料：迁移前项目表原本为空，迁移后保持空，不从任务文字猜测或生成项目。功能测试仍只写合成数据；正式验证强制浏览器 GET-only。

## 数据合同

`project_profiles` 每项目最多一条当前资料；新增事件 `project.profile_updated` 使用原 `project_history`。

| 字段 | 含义/约束 |
| --- | --- |
| summary | 简介，trim 后 1–4000 Unicode 字符 |
| architecture | 架构与入口，1–8000 字符 |
| development | 开发与验证方法，1–8000 字符 |
| evidence | 核实依据，1–4000 字符，可写仓库路径、文档/任务结论及说明 |
| sourceTaskId | 正整数，写入时必须为该项目的关联任务 |
| sourceTaskVersion | 正整数，必须等于写入时该任务的当前 version |

所有字段必填、不接受 null/未知字段；文本拒绝 NUL。CLI 输入文件/stdin 上限 **512 KiB**，读取阶段即限流；JSON 解析错误仅返回错误类别、行列，不回显非法值。

返回资料还含 `projectId`、`revision`、`updatedAt`。资料 revision 是最近一次资料维护时的项目 revision，不是独立计数器；项目改名/组件/源码变更后，当前项目 revision 可以更高。

全量替换，不是 Patch：不能遗漏字段来表示保留，也不能用空串清除。更新必须保留仍有效的原资料。相同内容再次显式提交也视为一次维护，递增 revision 并记录依据；失败后不能自动改用最新 revision 重试。

一个写事务内完成：

1. 检查项目 revision。
2. 检查来源任务存在、当前项目归属及 version。
3. 写当前资料、递增项目 revision、记录完整 `before`/`after` 以及来源任务/版本/依据。
4. 任一步失败全部回滚；不修改来源 Task、Task History 或 Session。

来源任务版本为历史快照，后续任务推进或改归属不会改写既有资料。引用一致性**不能证明内容真实**，也不构成系统自动验真或操作授权。

## CLI 使用

以下仅为独立沙箱示例。使用本轮开发二进制；不要替换全局安装，不向正式库执行。

```powershell
$cli = '.\.local\task40-target\debug\taskctl.exe'
$db = 'E:\sandbox\steward-profile\state.db'

# 项目及来源任务须已在这个隔离库中存在
& $cli --database $db --json project show '##1'
& $cli --database $db --json task context 1
& $cli --database $db --json project profile show '##1'

# revision 和来源任务版本必须来自刚核实的读取结果
& $cli --database $db --json project profile set '##1' --if-revision 3 --input profile.json
& $cli --database $db --json project history '##1' --after 0 --limit 50
```

`profile.json` 示例（数值仅为说明，不可盲用）：

```json
{
  "summary": "经核实的项目用途与边界",
  "architecture": "主要模块与入口文件，以及它们之间的调用关系",
  "development": "已确认的开发工具链与隔离验证命令；不把某次测试结果固化为长期事实",
  "sourceTaskId": 1,
  "sourceTaskVersion": 7,
  "evidence": "来源任务中的核实结论及相关源码/文档路径；说明尚未确认的边界"
}
```

AI 维护流程：先读当前项目资料、revision 和来源任务上下文，再核实依据；确定的事实直接维护，有疑问或内容冲突先问用户。遇到 `VERSION_CONFLICT` 重新读取并重新判断，不盲目重试。不记录 Token、Cookie、密码、授权头或隐藏推理，不承诺自动识别/过滤秘密。

长期资料应描述用途、架构、入口、开发验证方法；不要存“工作树 clean”“服务正在运行”“测试当前通过”等易失状态作为长期事实。

## 读取与上下文

- `project profile show` → `{project, profile}`；无资料 `profile:null`。
- 现有 `project show` / `GET /api/projects/{id}` 也返回 `profile`。
- `task context` / `GET /api/tasks/{id}/context` 在同一数据库读事务内带 `projectProfile`。
- 既有项目历史读取展示完整来源与 before/after，按 revision 分页。
- React 项目详情、任务概览和复制上下文显示资料及来源；旧后端缺少字段与明确 null 区分。
- 不把项目资料混入源码导航指纹或声称为实时源码观察；现场和任务状态仍须另行核验。

## Schema4 → 5 离线复制

明确入口：

```powershell
& $cli --json --yes --database 'E:\sandbox\new-private\schema5.db' database import-schema4 --source 'E:\sandbox\snapshots\schema4.db'
```

前提：可信本地绝对路径、停止写入并取得一致 SQLite 快照、独立私有目标目录、目标主文件与 WAL/SHM/journal 全不存在。命令不停止服务、不替换源、不切默认库、不安装。不能把运行中 WAL 数据库的主文件单独复制当作一致快照。

实现先冻结真实 Schema4 SQL（`crates/application/src/migration/schema4.sql`），再将当前初始化版本改为5；与旧 Schema2 复制共享既有安全流程：

- 源只读打开，完整布局、完整性/FK、身份及 data_version 核查。
- Schema4 复制13张既有表；9类 AUTOINCREMENT 高水位包括空表和已删除 ID 范围。
- task_components 以复合键确定顺序，逐字段对比并输出 typed-rows-v1 哈希；JSON/BLOB 原字节保留。
- 项目名称、历史 JSON、登记身份等只解码持久化数据；不运行 Git、不观察历史源码/Worktree/导入路径。
- Schema5 资料表保持空，不合成项目事实，不递增 Task/Project 版本，不添加业务 History。
- 私有 staging 完成后独占 hardlink 发布，不覆盖占用路径，也不采纳/清理上次中断遗留 staging。

`sourceQuiescenceVerified:false` 保留：快照、身份/data_version 复查不是并发写入栅栏，也不承诺断电持久性或抵御不可信目录的并发替换。操作者仍负责离线、可信目录和后续切换前复核。

`import-schema2` 仍只接受真实 Schema2，目标改为当前 Schema5；不能混用 `import-schema4` 自动识别。Schema4/2 正常启动在监听前拒绝，显式复制到新库后才可用候选二进制启动。

## 本轮验证

- 四包 `steward-application/storage-sqlite/taskctl/steward-server`：174 项 Rust 测试通过，包含资料 CAS/来源校验/History 故障回滚、CLI 错误不回显及输入上限、Schema2/4复制与进程中断/目标碰撞、旧库拒绝启动与新库重启、reader GET资料/上下文及无新 HTTP 写入口。
- Node24.11.1：62 项 Vitest、tsc、三文件构建通过。
- 新编译 `.local/task40-target/debug` 二进制的 Chrome 隔离 smoke 通过：资料/来源出现在项目和任务/复制上下文中，SSE/版本提示保护、无业务按钮/POST，Task/Project/Profile/History 均不变。
- 截图：`web/.artifacts/2026-09-09T03-39-33.692Z/`。所有数据为临时合成数据；专属 shutdown marker 退出，无测试 taskd 残留。
- 上述为资料开发阶段的验证记录；正式发布前的后续复验见下一节。LSP默认服务器缺失，类型验证使用实际 tsc。

## 本机发布记录与恢复边界

- 发布前实际备份预演发现冻结 Schema4 SQL 为 CRLF，而部署DDL来自 Rust LF；严格布局检查拒绝，旧服务未停止。仅固定冻结文件 LF 并新增回归和 `.gitattributes`，没有放宽布局比较；首个失败候选及快照保留。
- 修复后四包 **175 项 Rust**、**62 项 Vitest**、类型/构建门禁通过；Schema2真实双版本CLI合成演练通过。PowerShell7和5.1的隔离启动/打包/更新拒绝/回退测试通过；最终 release 二进制 Chrome 外部 UI 与内嵌 UI 两模式均通过。
- 最终停写后使用 SQLite Backup API 取一致快照，经公开 `database import-schema4` 复制；13表逐字段 typed hash 和序列高水位完全相等。保留42任务、43会话、121 Checkpoint、114备注、441历史、27导入；项目相关表原为空。迁移及正式只读验证期间 #40 一直 version24。
- 旧PID11776正常退出，新PID15044使用新安装版本与原IP/端口。启动执行包装器因长驻后代管道未关闭而超时；独立确认进程/监听/HTTP健康后仅完成验证，没有重复迁移、重复启动或强杀。独立正式 Chrome 验证 GET-only、真实#40详情、项目空状态、无业务按钮/页面异常，随后再次检查13表不变。
- 活动 UI release：`90a034c3478911ab75372cdb48c6b2261e1b4b1b61c1d68ad34c4a3615e3c75a`。只读内嵌 release：`embedded-44b851b60e34f3152a2171d93fd917e5a788ee0776220d99a80f146d636f2a38`。
- 私有恢复目录：`%LOCALAPPDATA%/agent-steward/backups/schema5-upgrade-20260909-070132`。包含最终 `migration/schema4-snapshot.db`、原主文件/sidecar、旧PATH三件套、配置/入口和冷runtime；凭据不得复制进任务记录或报告正文。
- 部署阶段及摘要：`.local/task40-release-state.json`；验证日志 `.local/task40-release-final-*.log`；正式页面截图 `.local/task40-formal-ui.png`。这些是本地证据，不是新的业务状态源。

已恢复当前会话的 Steward 记录，后续会产生新写入。**不能直接用旧备份覆盖新库，也不能只换回 Schema4 程序。** 如需恢复，必须重新停写、保留两侧数据并明确对账/恢复方案。旧安装与备份仅用于受控恢复，不继续向旧库分流写入。本机上线不等于完整人工业务验收，也不自动关闭 #40。
