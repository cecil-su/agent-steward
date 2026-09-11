# CLI 命令设计

Project 的 `##ID`/唯一名称、`project create/show/list/rename/history`、`task create/list --project`、`task project/components`、`project component/source/here/context` 及context字段见 [项目与上下文复用合同](16-项目与上下文复用.md)。当前数据库Schema7 的普通连接只初始化空库，不隐式升级旧库；JSON envelope为2。`rule` CLI、task context/sessionRules、Schema6显式复制和UI合同4见[个人偏好与项目规则](22-个人偏好与项目规则.md)。本文其余部分保留 Task/Session/Worktree 基础合同。

## 1. 通用约定

```bash
taskctl [global-options] <domain> <action> [subaction] [arguments] [options]
```

全局选项：

- `--database <path>`：覆盖默认 SQLite 数据库路径；
- `--json`：输出稳定机器合同，帮助、版本及解析失败也遵循JSON输出；仅`--`之前独立出现的选项生效，`--`之后的字面值及`--name=--json`选项值不启用JSON；JSON模式从不交互询问，需确认的操作必须显式`--yes`；
- `--input <file|->`：从 UTF-8 JSON 普通文件读取结构化输入，通用输入最多16 MiB（含边界）；拒绝目录、设备与命名管道文件路径，打开后再次检查文件类型。与`--json`同用时，`-`按原始字节流式读取stdin，最多读取限额+1字节即拒绝超限，不等待超限流EOF；限额内的stdin仍须由发送方结束输入。Rule/Profile输入保留各自更严格的专用限额。超限、空输入、非法UTF-8、非法JSON和未知字段返回`INVALID_INPUT`（`details.field="input"`）；输入文件不成为主存储；
- `--yes`：确认 Worktree 删除等明确的本地操作；
- `--verbose`：成功解析命令后向stderr输出构建版本与JSON模式诊断，不输出输入正文、凭据或参数值，不改变stdout结果合同。

默认数据库位于当前操作系统的用户级应用数据目录下，文件名为 `agent-steward/steward.db`。初版不提供远程 Git 写入命令，也不连接远程数据库服务。

### 显式离线归档迁移

`taskctl --database <不存在的新库绝对路径> --json --yes database import-v7 --source <schema_migrations格式v7快照绝对路径>` 只读源库，保留闭合任务的 ID、版本、时间戳及全部业务记录。拒绝活动任务、Worktree 绑定和既有目标。它不是 Task mutation，不生成新业务 History、不切换默认库。完整限制和验收见 [v7归档迁移](13-v7归档迁移.md)。

### 显式 Schema 2 快照复制

`taskctl --database <不存在的新库绝对路径> --json --yes database import-schema2 --source <停写Schema2快照绝对路径>` 支持活动/关闭任务、Session/Checkpoint、Import 和 Hook 墓碑。源只读，目标须有私有父目录（不存在时仅新建最后一级），禁止覆盖、合并、默认库回退或服务切换。返回七表计数/摘要与四类序列高水位；`sourceQuiescenceVerified:false` 明确不能以读事务或并发复查代替停写。Schema 3 不在支持范围。全部现行入口、输入格式和隔离步骤见[Schema7隔离导入与验证](18-Schema7隔离导入与验证.md)。

## 2. Task

```bash
taskctl task list [--status in_progress] [--task-key <key>] [--query <text>]
  [--page-size <1..200>] [--cursor <cursor>] [--fields <field,...>]
  [--format table|lines]
taskctl task show <task-ref> [--json]
taskctl task create [task-key] [--input <file|->]
taskctl task claim <task-ref> --session <session-id> --if-version <version> [--take-over]
taskctl task update <task-ref> --if-version <version> --input <file|-> --yes --reason <修改依据>
taskctl task retitle <task-ref> --if-version <version> --title <MMDD｜类型｜主题>
taskctl task note <task-ref> --if-version <version> --type <decision|progress|risk> --text <text>
taskctl task block <task-ref> --if-version <version> --reason <text> --recovery <text>
taskctl task unblock <task-ref> --if-version <version> --next-step <text>
taskctl task checkpoint <task-ref> --session <session-id> --if-version <version> --input <file|->
taskctl task resume <task-ref> --session <new-session-id> --if-version <version> [--from-session <old-session-id>] [--take-over] [--json]
taskctl task close <task-ref> --if-version <version> --outcome <outcome> [--reason <text>]
```

`<task-ref>` 接受纯数字 `12`、展示形式 `#12` 或可选 `taskKey`。前两种解析为整数主键，`taskKey` 按原文查询，没有 `key:` 转义语法。JSON 中的 `TaskView.id` 和所有 `taskId` 是整数；人类输出显示 `#12`。`taskKey` 不能为纯数字或以 `#` 开头；可以在创建时设置，也可以从 `null` 设置一次，之后不可更改或清空。

`task list` 默认每页50条，允许1–200条，页大小在打开数据库前校验，固定按 `updatedAt DESC, id ASC` 排序，并使用 `nextCursor` 继续读取。游标保存固定长度的筛选摘要并绑定创建它时的 `status/taskKey/query` 条件，不内嵌完整筛选文本；因此合法输入产生的 `nextCursor` 一定可被下一页消费，筛选条件变化后复用旧游标返回 `INVALID_INPUT`。`--status` 和 `--task-key` 精确匹配。`--query 45` 或 `--query '#45'` 按整数ID精确搜索，不匹配其它任务正文中的数字；前导零按同一编号处理，超出整数范围返回 `INVALID_INPUT`，不存在编号返回空列表。编号查询仍与状态、项目等筛选取交集；已关闭任务需选对应视图或不限制状态。其它 `--query` 对 title/goal/scope 做转义后的 SQLite `LIKE` 包含匹配，ASCII 字母不区分大小写，非 ASCII 遵循 SQLite 默认比较语义，`%` 和 `_` 按普通字符处理。`--fields title` 或 `--fields id,title,status` 只投影白名单字段；不传时 JSON 返回完整 TaskView。未知、重复或空字段拒绝，字段白名单就是下文 `TaskView` 的 camelCase 字段集合；投影不改变筛选、排序和游标计算。

非 JSON 的 `task list` 默认输出 ID/title/status/updatedAt 表格；显式 `--fields` 决定表格列，`null` 显示为 `—`，过长单元格只在表格中以省略号截断。`--format lines` 要求恰好选择一个字段，每条 Task 输出一行；它不能与 `--json` 组合。机器调用始终使用 `--json`，不解析表格或 lines 文本。

除 `task create` 外，所有会改变 Task、Session、Checkpoint、Note、History 或 Worktree 引用的命令都必须携带最近一次读取结果中的 `--if-version`。Application Service 使用该值执行 compare-and-swap；不匹配时返回版本冲突和当前版本，不执行部分 mutation。纯查询不需要版本。

不带 task-key 和 `--input` 的 `task create` 创建最小 Task：数据库生成整数 ID，`version=1`、`status=open`，描述和 `taskKey` 均为 `null`。位置参数可提供 `taskKey`；`create --input` 还可使用以下完整输入，`taskKey` 可选，描述字段可省略或为 `null`，非 `null` 时必须是去除首尾空白后仍非空的字符串；未知字段作为 Schema 错误拒绝，不能静默忽略：

```json
{
  "taskKey": "LOGIN-REGRESSION",
  "title": "0904｜修复｜登录回归",
  "goal": "恢复登录并保留现有会话兼容性",
  "scope": "仅修改认证模块和对应测试",
  "acceptanceCriteria": "相关单元测试和集成测试通过",
  "nextStep": "先补充失败用例"
}
```

`title` 非空时统一使用 `MMDD｜类型｜主题`，分隔符必须是全角 `｜`，`MMDD` 必须是有效月日，类型只能是 `功能`、`设计`、`修复`、`优化`、`发布`、`探索`、`文档` 或 `研究`，主题不能为空或带首尾空白。调用方负责按会话时间转换到 `Asia/Shanghai` 后生成 `MMDD`；核心校验结构、月日和类型，不从机器时区猜测日期。

`update --input` 是 JSON Merge Patch 风格的受限字段更新，只允许 `taskKey`、`title`、`goal`、`scope`、`acceptanceCriteria`、`nextStep`、`project` 和 `components`。必须显式 `--yes --reason <非空修改依据>`；HTTP `task-update` 必须携带 `confirmed:true`、`reason`，应用层也校验。支持全部状态的信息维护，不领取或修改 Session。`project` 为项目引用字符串或 null（清除），`components` 为最终项目内组件名称数组，[] 清空。换项目且存在旧组件时必须显式提供 components，不静默丢弃。closed 的 nextStep 仍必须为空。详见 [任务信息维护](20-任务信息维护.md)。省略表示不修改；`nextStep` 可显式为 `null`，字符串值必须非空白。四个描述字段初始可为 `null`，但设置为字符串后不能通过 `null` 清空；清空请求返回 `INVALID_INPUT` 且不递增 version。`taskKey` 仅允许从 `null` 设置为非空、非纯数字且不以 `#` 开头的字符串，之后不可改变；空 Patch、无实际变化的 Patch 和未知字段均拒绝。单次 Patch 无论改变多少字段，都只执行一次 CAS、递增一次 version 并写一条 `task.updated` History。状态、阻塞和关闭结果只能通过专用命令改变。

`retitle` 保留为只改标题的兼容入口；经授权的 `update` 同样允许维护已关闭 Task 的信息；Import/Hook 清理和 Worktree 管理仍按专用合同校验并可能递增 Task version，不改变关闭结果。它只接受符合命名规则的非空 title，执行一次 CAS，仅修改 `title`、`version` 和 `updatedAt`，写入一条 `task.retitled` History；不得重新打开 Task、改变关闭结果或修改其他字段。相同 title 作为无实际变化请求拒绝。未关闭 Task 也可使用该命令做 title-only 修正。

`claim` 的语义是把当前 Session 记录为 Task 的执行会话，并将 `open` Task 推进到 `in_progress`。请求的 Session ID 不存在时，命令在同一事务中创建未结束 Session；它已经存在时，仅允许它是该 Task 尚未结束的当前 Session，并按下述规则返回 no-op，不能重新激活已结束 Session、复用其他 Task 的 Session 或改写既有继续关系，否则返回 `SESSION_CONFLICT`。相同当前 Session 再次领取时，Application Service 先执行 version 校验；只有调用方携带当前 version 时才返回同状态 no-op success，不写 History、不递增 version。首次成功后丢失响应并原样重试旧 version 会返回 `VERSION_CONFLICT`，V0 不把它称为请求级幂等。如果已有其他当前 Session，默认返回冲突；用户可以明确使用 `--take-over` 接管。接管使用尚不存在的新 Session ID，把新 Session 的 `continuedFrom` 指向旧 Session，但不会伪造旧 Session 的 `endedAt`。

`resume` 会：

1. 解析来源 Session：优先使用 `--from-session`，否则使用当前 Session；两者都不存在时拒绝；
2. 校验来源 Session 属于同一 Task；新 Session ID 必须尚不存在且不能等于来源 Session ID，已存在、已结束或属于其他 Task 的 Session 都不能作为 resume 目标；
3. 若 Task 存在活跃当前 Session，无论它是否等于来源，都必须显式使用 `--take-over`；
4. 将来源 Session 保留在历史中，创建新 Session，并把 `continuedFrom` 指向来源 Session；
5. 读取最新 Checkpoint；
6. 实时观察 Worktree 和 Git；
7. 返回恢复上下文和唯一下一步。

`resume --session` 与来源 ID 相同时返回 `INVALID_INPUT`；目标 ID 已存在时返回 `CONSTRAINT_VIOLATION`，`details.constraint="sessions.id.unique"`；来源不存在时返回 `NOT_FOUND`。这些失败都发生在创建新 Session 或更新 Task 之前。

`checkpoint` 输入至少包含：

```json
{
  "summary": "当前进展摘要",
  "completed": [],
  "decisions": [],
  "pending": [],
  "nextStep": "下一步",
  "risks": []
}
```

`summary` 和 `nextStep` 必须是非空字符串，四个数组字段的每个元素也必须是非空字符串；未知字段拒绝。Checkpoint 一经写入不可原地修改，修正通过创建新 Checkpoint 完成。

状态转换如下；待上线语义详见[待上线任务状态](21-待上线任务状态.md)：

| 当前状态 | 命令 | 下一状态 | 附加规则 |
| --- | --- | --- | --- |
| `open` | `claim` | `in_progress` | 登记当前 Session |
| `in_progress/pending_release/blocked` | `claim --take-over` | 原状态 | 更换当前 Session 并保留继续关系 |
| `in_progress` | `block` | `blocked` | reason 和 recovery 必填 |
| `blocked` | `unblock` | `in_progress` | 清空阻塞字段，next step 必填 |
| `in_progress` | `pending-release` | `pending_release` | 功能开发完成、尚未上线；保留当前Session |
| `pending_release` | `continue` | `in_progress` | 继续修改，保留当前Session |
| `in_progress/pending_release` | `close completed` | `closed` | title、goal、scope、acceptanceCriteria 必须完整；清空当前 Session 和 next step |
| `in_progress/pending_release/blocked` | `close partial` | `closed` | reason 必填并记录残余事项 |
| `open/in_progress/pending_release/blocked` | `close cancelled/superseded` | `closed` | reason 必填 |

`update` 允许用于全部状态，`note` 只允许用于 `open/in_progress/pending_release/blocked`；都不能绕过专用命令改变状态字段。`checkpoint` 只允许用于 `in_progress/pending_release/blocked`，且指定 Session 必须是同一 Task 尚未结束的当前 Session。`resume` 只允许用于 `in_progress/pending_release/blocked`；`open` Task 必须先 `claim`。`claim` 首次把 `open` 推进到 `in_progress`；对于 `in_progress/pending_release/blocked`，相同且尚未结束的当前 Session 在当前 version 下可以 no-op success，不同当前 Session 必须以尚不存在的新 Session ID 执行 `--take-over`，状态保持不变；当前 Session 为空时必须使用带明确来源的 `resume`，不能丢失继续关系。

`closed` 不允许重新领取、阻塞或保存 Checkpoint；V0 不提供 reopen，但允许经授权的 `update/project/components` 信息维护以及 `retitle` 标题修正。所有关闭命令都清空 next step 和阻塞字段；若存在当前 Session，还在同一事务中设置其 `endedAt` 并清空 Task 的 `currentSessionId`。非 `blocked` 状态的阻塞字段必须为空；非 `closed` 状态的关闭字段必须为空。

## 3. Session

```bash
taskctl session list [--task <task-ref>]
taskctl session show <session-id>
taskctl session attach <task-ref> --session <session-id> --if-version <version> [--source <client>] [--external-session <external-id>] [--record-path <path>]
taskctl session import add <task-ref> --session <session-id> --if-version <version> --file <session-file> --confirm-sensitive-content-reviewed
taskctl session import list <session-id> [--json]
taskctl session import remove <import-id> --if-version <version> [--yes]
taskctl session close <session-id> --if-version <version>
```

初版的 Session 只保存关联信息、来源和可选记录路径。完整聊天记录仍由 AI 客户端负责；`attach --record-path` 对已存在文件保存规范化绝对路径，对不存在文件保存展开后的绝对弱引用；`import add` 只在用户明确提供文件时把可观察内容和哈希复制到 SQLite。

`attach` 创建本地 Session，或者在全部已提供身份字段一致、调用方携带当前 version 时返回 no-op success；no-op 不写 History、不递增 version。提供 `externalSessionId` 时也必须提供非空 `source`，两者组合在外部 ID 非空时全局唯一。`import add` 要求目标 Session 已存在并属于指定 Task，来源使用该 Session 已保存的 `source`，不从文件内容猜测或创建 Session。`close` 设置 `endedAt`；若它是 Task 的当前 Session，则在同一事务中清空 `currentSessionId` 并写入 History，但不改变 Task 状态。

`import add` 只接受可确定大小的普通文件，解析前上限为 16 MiB；拒绝目录、设备、Socket 和 FIFO。实现必须使用固定大小缓冲区边读取边计算 SHA-256，并在超过上限时停止，不得先把无限输入完整载入内存。数据库无内容级加密；调用方必须在读取文件和数据库 mutation 前携带 `--confirm-sensitive-content-reviewed`，否则返回 `INVALID_INPUT`，其 `details.field=confirmSensitiveContentReviewed`。成功响应仍返回稳定警告码 `SENSITIVE_CONTENT_CHECK_REQUIRED` 作为安全提示。

相同 `sessionId + sha256` 只保存一份。调用方携带当前 version 重复导入时返回现有 `SessionImportView`、警告 `DUPLICATE_SESSION_IMPORT`，不更新 `sourcePath`、不写 History、不递增 version；携带旧 version 仍先返回 `VERSION_CONFLICT`。`import list` 只返回元数据，不返回 BLOB 内容。

`import remove` 在事务中根据 Import ID 找到所属 Task、执行 version compare-and-swap、删除 BLOB 并写入 `session.import_removed` History。交互终端未提供 `--yes` 时显示 Import ID、Session ID、SHA-256 和大小并要求确认；非交互环境未提供 `--yes` 时拒绝。它只保证逻辑删除，物理擦除边界和 WAL 处理见安全文档。

如果客户端没有暴露外部 Session ID，调用方先生成一个稳定本地 ID（推荐 UUID）传给 `--session`，并将外部 ID 保留为空。CLI 不从聊天内容或文件名猜测外部 ID。

## 4. Worktree

```bash
taskctl worktree create <task-ref> --repo <path> --branch <branch> --path <worktree-path> --if-version <version>
taskctl worktree status <task-ref> [--json]
taskctl worktree remove <task-ref> --if-version <version> [--yes]
taskctl worktree adopt <task-ref> --repo <path> --path <worktree-path> --if-version <version>
taskctl worktree detach <task-ref> --expected-path <worktree-path> --if-version <version>
```

`create` 要求调用方显式提供目标路径。命令取得下述按 Task advisory lock 后，必须重新读取 Task version，并确认 Task 的 Repository 路径、common-dir 身份、Branch 和 Worktree 引用全部为空；任一引用已经存在时，在调用 Git 前返回 `WORKTREE_SAFETY_REFUSED`，不能覆盖或创建第二个未登记 Worktree。Git 调用完成后无论退出状态如何都必须重新观察现场；只有现场证明创建成功，才在数据库事务中以同一 version compare-and-swap，并同时保存实际 Repository 路径、规范化 common-dir 身份、Branch 和 Worktree 引用。数据库提交后、返回成功前必须再次确认 Worktree 仍存在且身份一致。`--branch` 表示已经存在的本地分支；V0 不隐式创建分支。目标分支不存在、已被其他 Worktree 占用或 Repository 身份不一致时拒绝。

Repository、Worktree 和目标父目录必须遵循安全文档中的 `CanonicalPath` 与 `RepositoryIdentity` 合同。数据库路径相等、用户输入字符串相等或单独一次 `resolve()` 都不足以证明是同一现场。

`create/remove/adopt/detach` 在读取最终前置条件前，必须取得跨进程 OS advisory lock，并持有到 Git 后置观察和数据库提交或错误分类完成。锁文件位于用户应用数据目录的 `agent-steward/locks`，文件名使用 `SHA-256(canonicalDatabasePath + NUL + 数字 taskId)`，目录权限与数据库应用目录相同；文件内容不保存业务状态，进程退出后由操作系统释放锁。锁不可用时返回 `WORKTREE_OPERATION_BUSY`，不得启动 Git。该锁只串行化同一数据库中同一 Task 的 Worktree 外部操作，Task version CAS 和数据库唯一约束仍是持久化一致性的最终保护；V0 不支持多个 OS 用户共享同一数据库。

最低保护：

- 创建前检查仓库、目标分支和路径；
- 创建前拒绝任何已经登记 Repository/Worktree 引用的 Task；
- 状态命令实时读取 HEAD、dirty、staged、untracked 和 ignored；
- dirty Worktree 默认拒绝删除；
- 不提供隐式 force、clean、reset、stash 或 push；
- 删除成功后才清除 Task 中的 Worktree 引用，并在数据库提交后再次确认路径和 Git 登记都未出现。

`remove` 在交互终端且未提供 `--yes` 时显示规范化目标路径并要求确认；非交互环境未提供 `--yes` 时直接拒绝，不得等待 Prompt。

Git 与 SQLite 部分完成时使用显式恢复命令：

- `adopt` 仅在 Git 实时证明该规范化路径是指定 Repository 已登记的 Worktree、且 Task 当前没有 Worktree 引用时，保存实际 Repository 路径、common-dir 身份、Branch 和 Worktree 引用；提交前必须再次复核，提交后返回成功前也必须确认引用仍存在，否则返回 `PARTIAL_EXTERNAL_STATE`；它不创建、不移动也不修改 Worktree；
- `detach` 仅在 `--expected-path` 与数据库中的规范化路径完全一致，且 Git 与文件系统证明该 Worktree 已不存在时清除陈旧引用；提交前必须再次复核，提交后返回成功前也必须确认引用未被外部 Git 重建，否则返回 `PARTIAL_EXTERNAL_STATE`；它不删除任何文件；
- 两个命令都必须执行 Task version compare-and-swap、写入 History，并在现场无法证明安全时拒绝；
- `doctor` 只给出诊断和建议命令，不自动调用 `adopt` 或 `detach`。

每次 `git worktree add/remove` 启动后，无论进程退出码是成功还是失败，都必须重新观察 CanonicalPath、RepositoryIdentity 和 Worktree 登记状态。只有能够证明现场未改变时，非零退出才返回 `GIT_COMMAND_FAILED`。如果现场已经改变而数据库尚未提交，返回 `PARTIAL_EXTERNAL_STATE` 和 `adopt`/`detach` 建议；`create`已创建Worktree但Task引用仍为空时，只有重新读取Task并验证恢复前置条件成功，才建议携带最新version的adopt；连接或重读失败时必须停止，建议doctor并明确恢复连接后重新读取Task与Git，禁止发出旧版本恢复命令。如果Git已经启动且无法证明现场是否改变，同样返回`PARTIAL_EXTERNAL_STATE`，其中`gitState=null`，建议命令为 `taskctl doctor`，不能降级为 `PATH_IDENTITY_UNKNOWN` 或普通 Git 错误。数据库已经提交而后置观察发现不一致时，`databaseState` 必须为 `updated`。

## 5. History 与诊断

```bash
taskctl history <task-ref> [--json]
taskctl doctor
```

History 与对应 mutation 在同一 SQLite 事务中写入，记录 Task 创建、领取、更新、Note、阻塞、Checkpoint、Session attach/resume/close、Session Import add/remove、Worktree 引用变化和 Task 关闭；同状态 no-op 不写 History。History 不承担复杂安全审计，稳定变更类型和最低 payload 见数据模型文档。

`doctor` 至少检查数据库可打开、schema 版本受支持、外键一致性、SQLite `quick_check`、记录路径提示和已登记 Worktree 引用；它不能把数据库引用当作 Git 现场事实。

## 6. AI 会话更新协议

AI 应遵守：

1. 新窗口读取任务需求时运行 `task context --json`，同时取得当前 Task version 与完整有效 sessionRules；只读查询不自动领取任务；
2. 使用该 version 执行 `task claim` 或 `task resume`，并从成功结果取得新 version；
3. 状态、关键决策或阻塞变化时调用对应命令，每次成功后继续携带最新 version；
4. Git 现场使用 `worktree status` 获取；
5. 会话结束或上下文即将耗尽前保存 Checkpoint；
6. 遇到版本冲突时重新 `show`，不得盲目重试旧 Patch；
7. 不直接修改 SQLite 数据库。

这些规则可以写入项目 `AGENTS.md`。Client Hook只提交可观察元数据，不能替代显式Task更新或改变Session执行生命周期。

## 7. JSON 合同

`--json` 时 stdout 只输出一个 UTF-8 JSON 对象并以换行结束；诊断信息只能写入 stderr。`--help`（含子命令帮助）与 `--version` 同样返回成功 envelope，data 分别为 `{ "help": "帮助文本" }` 和 `{ "version": "版本文本" }`，退出码0，不打开数据库；未指定 `--json` 时保留终端文本。所有结果使用 camelCase 和固定 envelope：

```json
{
  "schemaVersion": 2,
  "ok": true,
  "data": {},
  "warnings": [
    { "code": "STABLE_WARNING_CODE", "message": "人类可读说明", "details": {} }
  ],
  "error": null
}
```

失败时 `ok=false`、`data=null`，`error` 至少包含 `code`、`message`、`retryable` 和 `details`。可选字段必须显式输出为 `null`，不能因为空而省略；输出消费者必须忽略未来新增字段。输入中的未知字段拒绝，以防拼写错误静默丢失。持久化 Checkpoint 或 History JSON 无法解码时返回 `DATABASE_UNAVAILABLE`，不得转换为空数组或 `null`；`resume` 必须在创建新 Session 前完成 Checkpoint 解码。

所有成功的 Task mutation 在 `data.task` 中返回完整 Task，其 `version` 是 mutation 后的新版本。`VERSION_CONFLICT` 的 `details` 返回 `expectedVersion` 和 `currentVersion`。Git 已经改变现场，或操作期间无法证明 Git 与数据库一致时，返回 `PARTIAL_EXTERNAL_STATE`；`details` 返回规范化 Repository/Worktree 路径、完整观察对象或`null`的Git状态、稳定的`databaseState`（`unchanged`、`updated` 或 `unknown`），以及可执行的 `adopt`、`detach` 或 `taskctl doctor` 建议。`schemaVersion:2`中的Task主键和taskId为JSON整数，taskKey及描述遵循可空合同。`recommendedCommand` 保持字符串；`recommendedArgs` 是不经过 Shell 拼接的参数数组，首项为 `taskctl`，并显式携带 `--database` 和规范化数据库路径；数字 Task ID、Repository 与 Worktree 路径各自占用独立数组元素。

`gitState`为可空观察合同：有完整观察时恰好包含`pathExists:boolean`、`registeredByGit:boolean`、`branch:string|null`、`head:string|null`；无法证明现场或只有先前完成动作时输出`null`，不能以完成动作代替观察。消费者必须先处理null。观察不构成随后操作的原子保证。`phase`、`observationError`、`lastObservedGitState`等诊断只放在`diagnostics`，不能把历史观察冒充实时状态；无诊断时显式输出null。`detach` 建议只在该 Task 的文件系统路径与 Git 登记均已证明不存在时给出；目录缺失但Git仍登记该Worktree，或存在无法排除的缺失登记别名时，不得建议detach；不同的缺失路径字符串不能证明不存在别名，应建议 `taskctl doctor` 并在 `diagnostics` 或 doctor issue 中说明需要先清理 Git 登记或恢复目录。

`databaseState=unchanged` 表示本次命令未提交数据库 mutation，`updated` 表示 mutation 已提交，`unknown` 表示提交结果无法确认；该字段不推断其他进程是否同时修改了数据库。

V0 稳定错误码固定为：

| code | 退出码 | retryable | `details` 最低字段 |
| --- | ---: | --- | --- |
| `INVALID_INPUT` | 2 | false | `field`、`reason` |
| `NOT_FOUND` | 2 | false | `entityType`、`id` |
| `UNSUPPORTED_SCHEMA_VERSION` | 2 | false | `databaseVersion`、`supportedVersion` |
| `VERSION_CONFLICT` | 4 | true | `expectedVersion`、`currentVersion` |
| `SESSION_CONFLICT` | 4 | false | `currentSessionId`、`requestedSessionId` |
| `DATABASE_BUSY` | 4 | true | `timeoutMs` |
| `WORKTREE_OPERATION_BUSY` | 4 | true | `taskId`、`operation` |
| `CONSTRAINT_VIOLATION` | 4 | false | `constraint` |
| `WORKTREE_SAFETY_REFUSED` | 5 | false | `reason`、`worktreePath` |
| `PATH_IDENTITY_UNKNOWN` | 5 | false | `inputPath`、`reason` |
| `GIT_COMMAND_FAILED` | 5 | false | `operation`、`exitStatus`、`stderrSummary` |
| `PARTIAL_EXTERNAL_STATE` | 6 | false | `repositoryPath`、`worktreePath`、`gitState`、`databaseState`、`diagnostics`（无诊断时为 `null`）、`recommendedCommand: string`、`recommendedArgs: string[]` |
| `DATABASE_UNAVAILABLE` | 10 | false | `reason` |
| `GIT_READ_LIMIT` | 5 | false | `{}`；message区分截止时间、取消或输出超限 |

`retryable=true` 只表示重新读取状态或等待后重试可能成功，不授权自动覆盖、接管或执行 destructive 操作。V0 稳定警告码至少包括 `SENSITIVE_CONTENT_CHECK_REQUIRED`、`DUPLICATE_SESSION_IMPORT`、`PHYSICAL_ERASURE_NOT_GUARANTEED`、`RECORD_PATH_MISSING` 和 `INSECURE_DATABASE_PERMISSIONS`；Warning 的 details 也必须使用机器字段，不能只返回自然语言。

V0 DTO 固定如下；这里列出的可选字段也必须以 `null` 输出：

| DTO | 字段 |
| --- | --- |
| `TaskView` | `id`（整数）、`taskKey`、`title`、`status`、`version`、`goal`、`scope`、`acceptanceCriteria`、`nextStep`、`blockReason`、`blockRecovery`、`currentSessionId`、`repositoryPath`、`repositoryCommonDir`、`repositoryBranch`、`worktreePath`、`latestCheckpointId`、`closureOutcome`、`closureReason`、`closedAt`、`createdAt`、`updatedAt`；`taskKey/title/goal/scope/acceptanceCriteria` 可为 `null` |
| `SessionView` | `id`、`taskId`、`source`、`externalSessionId`、`continuedFrom`、`recordPath`、`startedAt`、`endedAt` |
| `CheckpointView` | `id`、`taskId`、`sessionId`、`summary`、`completed`、`decisions`、`pending`、`nextStep`、`risks`、`gitHead`、`createdAt` |
| `TaskNoteView` | `id`、`taskId`、`sessionId`、`noteType`、`text`、`createdAt` |
| `SessionImportView` | `id`、`sessionId`、`sourcePath`、`mediaType`、`sha256`、`sizeBytes`、`importedAt`；不回传 BLOB 内容 |
| `WorktreeStatus` | `registered`、`repositoryPath`、`repositoryCommonDir`、`path`、`exists`、`branch`、`head`、`staged`、`unstaged`、`untracked`、`ignored`、`observedAt` |
| `HistoryEntry` | `id`、`taskId`、`sequence`、`changeType`、`sessionId`、`occurredAt`、`summary`、`payload` |

`WorktreeStatus.registered/exists` 是布尔值。`repositoryPath`、`repositoryCommonDir`、`path` 和 `branch` 是数据库登记值：`registered=true` 时必须保留并返回，即使文件系统中的 Worktree 已经不存在；`registered=false` 时这些字段为 `null`，`exists=false`。`exists` 和 `observedAt` 来自实时观察；`head`、`staged`、`unstaged`、`untracked`、`ignored` 也是观察值，Worktree 不存在时分别为 `null`，不能伪造空 HEAD 或空数组。Worktree 存在时四个文件数组按 Repository 相对路径字典序排列。只读 `worktree status` 的 Git 命令失败返回 `GIT_COMMAND_FAILED`，读取资源超限返回 `GIT_READ_LIMIT`；Git mutation 启动后的后置观察无法证明现场未改变时返回 `PARTIAL_EXTERNAL_STATE`，二者都不能返回伪造状态。

命令的 `data` 映射固定为：

- `task show` 以及 `task create/claim/update/retitle/block/unblock/close`：`{ "task": TaskView }`；
- `task note`：`{ "task": TaskView, "note": TaskNoteView }`；
- `task checkpoint`：`{ "task": TaskView, "checkpoint": CheckpointView }`；
- `task list`：`{ "tasks": TaskView[]|object[], "nextCursor": string|null, "hasMore": boolean, "pageSize": integer }`；未指定 `fields` 时返回完整 TaskView，指定后每个 object 只含请求字段；排序固定为 `updatedAt DESC, id ASC`；
- `task resume`：`{ "task": TaskView, "checkpoint": CheckpointView|null, "sessions": SessionView[], "worktreeStatus": WorktreeStatus|null, "nextStep": string|null }`；`sessions` 按 `startedAt ASC, id ASC`；
- `session show/list`：分别为 `{ "session": SessionView }` 和 `{ "sessions": SessionView[] }`；`session list` 按 `startedAt ASC, id ASC`；
- `session attach/close`：`{ "task": TaskView, "session": SessionView }`；
- `session import add/remove`：`{ "task": TaskView, "import": SessionImportView }`，remove 返回删除前的元数据快照且不含 BLOB；
- `session import list`：`{ "imports": SessionImportView[] }`，按 `importedAt ASC, id ASC`；
- `worktree status`：`{ "worktreeStatus": WorktreeStatus }`；
- `worktree create/remove/adopt/detach`：`{ "task": TaskView, "worktreeStatus": WorktreeStatus }`；
- `history`：`{ "history": HistoryEntry[] }`，按 sequence 升序；
- `doctor`：`{ "checks": [{ "code": string, "status": "ok"|"warning"|"error", "details": object }] }`。

DTO 与 SQLite 字段分离，但字段含义必须一一映射；时间统一输出 UTC RFC 3339。Task ID、Session/Checkpoint/Note/History DTO 中的 `taskId` 和 Note/History 自增 ID 输出 JSON 整数；Session、Checkpoint、Import ID 与 SHA-256 输出字符串。人类模式中的 Task ID 显示为 `#<id>`。任何破坏兼容性的字段删除、改名或语义改变必须增加 `schemaVersion`；只新增字段时消费者仍必须能够忽略。

### Git 读取预算

所有Git读进程使用10秒预算、8MiB stdout与64KiB stderr流式上限，并终止所管理的进程组/Windows Job、回收直接子进程。Unix pipe采用非阻塞读取与停止标志，避免逃逸进程组的后代持有writer时reader join无限等待；这不保证杀死已经逃逸的Unix后代。CLI `task here/context`、`project here/context/source resolve`、`worktree status`、`doctor` 的同次查询共用一个总预算，不为每条Git命令重置。Task context/doctor保留观测失败警告或诊断语义，不将未知状态当作clean。Git写进程不受读取消作用域控制；mutation前后每次Git读取独立限时，后置失败仍按 `PARTIAL_EXTERNAL_STATE` 分类。该预算不是SQLite、原生文件I/O、进程清理或整个CLI操作的硬时间保证。

## 8. 退出码

- `0`：成功；
- `2`：输入或 Schema 错误；
- `4`：版本、Session、数据库繁忙、Worktree 操作占用或约束冲突；
- `5`：Worktree 安全检查、路径身份检查或已证明未改变现场的 Git 执行失败；
- `6`：Git 已改变现场，或操作期间无法证明 Git 与数据库一致，返回 `PARTIAL_EXTERNAL_STATE`；
- `10`：数据库不可用或内部错误。

## 日常查找与上下文交接

- `task list --view active|in-progress|blocked|recent` 提供固定视图：active 包含 open/in_progress/blocked，in-progress 和 blocked 精确筛选，recent 不筛选状态。全部按 `updatedAt DESC,id ASC` 排序，沿用分页、查询和字段投影；`--view` 与 `--status` 互斥。`--status active` 也可表示未关闭任务。
- 默认终端列表展示 ID、标题、状态、下一步、更新时间；显式 `--fields` 不变。
- `task here` 读取当前目录，优先返回包含该目录且属于当前 Git 仓库的已登记 Worktree 任务；否则列出同 common-dir 的候选。返回 `directory`、`matchedBy`（worktree/repository/none）、`tasks`，包含已关闭任务，不分页、不自动领取；无匹配也是成功空列表。
- `task context <task-ref> [--format markdown]` 为只读查询，默认 Markdown 输出；`--json` 返回 envelope，data 包含 `task`、`checkpoint`、`session`（当前 Session 或 null）、`worktreeStatus`。Task、Checkpoint、Session 在同一个读事务中取得快照，事务释放后观察 Git；不修改版本、Session、History。
- 上下文包括目标、范围、验收、最新 Checkpoint 的完成项/决策/待办/风险、当前下一步、阻塞恢复条件与工作目录；不自动导出完整 Session 历史、Import 内容或全部 Notes。无 Checkpoint 时明确留空，Git 无法观察时 `worktreeStatus=null`，附带 `WORKTREE_OBSERVATION_FAILED` 警告。
- 人类模式 `resume` 使用同一摘要格式；其既有创建 Session 和 CAS 语义不变。导出上下文不会执行 resume。

## Notes与Hook命令

```bash
taskctl task notes <task-ref> --json
taskctl session bind <session-id> --source <source> --external-session <external-id> --if-version <version>
taskctl --json --input <event.json> hook ingest
taskctl hook list <session-id> --after 0 --limit 100 --json
taskctl hook clear <session-id> --if-version <version> --yes
```

notes 是只读查询。bind 使用 CAS 并写 History；hook ingest 按标准事件键去重，不使用 Task version、不改变执行状态。clear 使用 CAS，保留去重标记；没有可见事件时为当前版本 no-op。完整输入、容量和原生投递边界见 [Hook与HTTP合同](11-Hook与HTTP合同.md)。
