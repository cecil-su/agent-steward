# CLI 命令设计

## 1. 通用约定

```bash
taskctl [global-options] <domain> <action> [subaction] [arguments] [options]
```

全局选项：

- `--database <path>`：覆盖默认 SQLite 数据库路径；
- `--json`：输出稳定机器合同；
- `--input <file>`：从 UTF-8 JSON 文件读取结构化输入，文件只作为本次命令输入，不成为主存储；
- `--yes`：确认 Worktree 删除等明确的本地操作；
- `--verbose`：输出诊断信息，不改变结果合同。

默认数据库位于当前操作系统的用户级应用数据目录下，文件名为 `agent-steward/steward.db`。初版不提供远程 Git 写入命令，也不连接远程数据库服务。

## 2. Task

```bash
taskctl task list [--status in_progress]
taskctl task show <task-id> [--json]
taskctl task create <task-id> --input <file>
taskctl task claim <task-id> --session <session-id> --if-version <version> [--take-over]
taskctl task update <task-id> --if-version <version> --input <file>
taskctl task note <task-id> --if-version <version> --type <decision|progress|risk> --text <text>
taskctl task block <task-id> --if-version <version> --reason <text> --recovery <text>
taskctl task unblock <task-id> --if-version <version> --next-step <text>
taskctl task checkpoint <task-id> --session <session-id> --if-version <version> --input <file>
taskctl task resume <task-id> --session <new-session-id> --if-version <version> [--from-session <old-session-id>] [--take-over] [--json]
taskctl task close <task-id> --if-version <version> --outcome <outcome> [--reason <text>]
```

除 `task create` 外，所有会改变 Task、Session、Checkpoint、Note、History 或 Worktree 引用的命令都必须携带最近一次读取结果中的 `--if-version`。Application Service 使用该值执行 compare-and-swap；不匹配时返回版本冲突和当前版本，不执行部分 mutation。纯查询不需要版本。

`create --input` 使用以下完整输入，其中前四个字段必须是去除首尾空白后仍非空的字符串，`nextStep` 可省略或为非空字符串；未知字段作为 Schema 错误拒绝，不能静默忽略：

```json
{
  "title": "修复登录回归",
  "goal": "恢复登录并保留现有会话兼容性",
  "scope": "仅修改认证模块和对应测试",
  "acceptanceCriteria": "相关单元测试和集成测试通过",
  "nextStep": "先补充失败用例"
}
```

`update --input` 是 JSON Merge Patch 风格的受限字段更新，只允许 `title`、`goal`、`scope`、`acceptanceCriteria` 和 `nextStep`。省略表示不修改，`nextStep` 可以显式为 `null`，其余字段不能为 `null` 或空字符串；空 Patch 拒绝。状态、阻塞和关闭结果只能通过专用命令改变。

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

状态转换固定为：

| 当前状态 | 命令 | 下一状态 | 附加规则 |
| --- | --- | --- | --- |
| `open` | `claim` | `in_progress` | 登记当前 Session |
| `in_progress/blocked` | `claim --take-over` | 原状态 | 更换当前 Session 并保留继续关系 |
| `in_progress` | `block` | `blocked` | reason 和 recovery 必填 |
| `blocked` | `unblock` | `in_progress` | 清空阻塞字段，next step 必填 |
| `in_progress` | `close completed` | `closed` | 清空当前 Session 和 next step |
| `in_progress/blocked` | `close partial` | `closed` | reason 必填并记录残余事项 |
| `open/in_progress/blocked` | `close cancelled/superseded` | `closed` | reason 必填 |

`update` 和 `note` 允许用于 `open/in_progress/blocked`，但不能绕过专用命令改变状态字段。`checkpoint` 只允许用于 `in_progress/blocked`，且指定 Session 必须是同一 Task 尚未结束的当前 Session。`resume` 只允许用于 `in_progress/blocked`；`open` Task 必须先 `claim`。`claim` 首次把 `open` 推进到 `in_progress`；对于 `in_progress/blocked`，相同且尚未结束的当前 Session 在当前 version 下可以 no-op success，不同当前 Session 必须以尚不存在的新 Session ID 执行 `--take-over`，状态保持不变；当前 Session 为空时必须使用带明确来源的 `resume`，不能丢失继续关系。

`closed` 不允许重新领取、更新、阻塞或保存 Checkpoint；V0 不提供 reopen。所有关闭命令都清空 next step 和阻塞字段；若存在当前 Session，还在同一事务中设置其 `endedAt` 并清空 Task 的 `currentSessionId`。非 `blocked` 状态的阻塞字段必须为空；非 `closed` 状态的关闭字段必须为空。

## 3. Session

```bash
taskctl session list [--task <task-id>]
taskctl session show <session-id>
taskctl session attach <task-id> --session <session-id> --if-version <version> [--source <client>] [--external-session <external-id>] [--record-path <path>]
taskctl session import add <task-id> --session <session-id> --if-version <version> --file <session-file> --confirm-sensitive-content-reviewed
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
taskctl worktree create <task-id> --repo <path> --branch <branch> --path <worktree-path> --if-version <version>
taskctl worktree status <task-id> [--json]
taskctl worktree remove <task-id> --if-version <version> [--yes]
taskctl worktree adopt <task-id> --repo <path> --path <worktree-path> --if-version <version>
taskctl worktree detach <task-id> --expected-path <worktree-path> --if-version <version>
```

`create` 要求调用方显式提供目标路径。命令取得下述按 Task advisory lock 后，必须重新读取 Task version，并确认 Task 的 Repository 路径、common-dir 身份、Branch 和 Worktree 引用全部为空；任一引用已经存在时，在调用 Git 前返回 `WORKTREE_SAFETY_REFUSED`，不能覆盖或创建第二个未登记 Worktree。Git 调用完成后无论退出状态如何都必须重新观察现场；只有现场证明创建成功，才在数据库事务中以同一 version compare-and-swap，并同时保存实际 Repository 路径、规范化 common-dir 身份、Branch 和 Worktree 引用。数据库提交后、返回成功前必须再次确认 Worktree 仍存在且身份一致。`--branch` 表示已经存在的本地分支；V0 不隐式创建分支。目标分支不存在、已被其他 Worktree 占用或 Repository 身份不一致时拒绝。如果未来需要创建分支，另行增加显式 `--new-branch` 和 `--start-point` 合同。

Repository、Worktree 和目标父目录必须遵循安全文档中的 `CanonicalPath` 与 `RepositoryIdentity` 合同。数据库路径相等、用户输入字符串相等或单独一次 `resolve()` 都不足以证明是同一现场。

`create/remove/adopt/detach` 在读取最终前置条件前，必须取得跨进程 OS advisory lock，并持有到 Git 后置观察和数据库提交或错误分类完成。锁文件位于用户应用数据目录的 `agent-steward/locks`，文件名使用 `SHA-256(canonicalDatabasePath + NUL + taskId)`，目录权限与数据库应用目录相同；文件内容不保存业务状态，进程退出后由操作系统释放锁。锁不可用时返回 `WORKTREE_OPERATION_BUSY`，不得启动 Git。该锁只串行化同一数据库中同一 Task 的 Worktree 外部操作，Task version CAS 和数据库唯一约束仍是持久化一致性的最终保护；V0 不支持多个 OS 用户共享同一数据库。

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

每次 `git worktree add/remove` 启动后，无论进程退出码是成功还是失败，都必须重新观察 CanonicalPath、RepositoryIdentity 和 Worktree 登记状态。只有能够证明现场未改变时，非零退出才返回 `GIT_COMMAND_FAILED`。如果现场已经改变而数据库尚未提交，返回 `PARTIAL_EXTERNAL_STATE` 和 `adopt`/`detach` 建议；`create` 已创建 Worktree 但 Task 引用仍为空时，必须重新读取 Task 并在建议中携带其当前 version，不能只建议无法发现孤立 Worktree 的 `doctor`。如果 Git 已经启动且无法证明现场是否改变，同样返回 `PARTIAL_EXTERNAL_STATE`，其中 Git 状态为 `unknown`，建议命令为 `taskctl doctor`，不能降级为 `PATH_IDENTITY_UNKNOWN` 或普通 Git 错误。数据库已经提交而后置观察发现不一致时，`databaseState` 必须为 `updated`。

## 5. History 与诊断

```bash
taskctl history <task-id> [--json]
taskctl doctor
```

History 与对应 mutation 在同一 SQLite 事务中写入，记录 Task 创建、领取、更新、Note、阻塞、Checkpoint、Session attach/resume/close、Session Import add/remove、Worktree 引用变化和 Task 关闭；同状态 no-op 不写 History。History 不承担复杂安全审计，稳定变更类型和最低 payload 见数据模型文档。

`doctor` 至少检查数据库可打开、schema 版本受支持、外键一致性、SQLite `quick_check`、记录路径提示和已登记 Worktree 引用；它不能把数据库引用当作 Git 现场事实。

## 6. AI 会话更新协议

AI 应遵守：

1. 开始时运行 `task show --json`，取得当前 Task version；
2. 使用该 version 执行 `task claim` 或 `task resume`，并从成功结果取得新 version；
3. 状态、关键决策或阻塞变化时调用对应命令，每次成功后继续携带最新 version；
4. Git 现场使用 `worktree status` 获取；
5. 会话结束或上下文即将耗尽前保存 Checkpoint；
6. 遇到版本冲突时重新 `show`，不得盲目重试旧 Patch；
7. 不直接修改 SQLite 数据库。

这些规则可以写入项目 `AGENTS.md`。后续 Client Hook / Runtime Adapter 可以自动提交 Session 生命周期和可观察事件，但不能替代显式 Task 更新。

## 7. JSON 合同

`--json` 时 stdout 只输出一个 UTF-8 JSON 对象并以换行结束；诊断信息只能写入 stderr。所有结果使用 camelCase 和固定 envelope：

```json
{
  "schemaVersion": 1,
  "ok": true,
  "data": {},
  "warnings": [
    { "code": "STABLE_WARNING_CODE", "message": "人类可读说明", "details": {} }
  ],
  "error": null
}
```

失败时 `ok=false`、`data=null`，`error` 至少包含 `code`、`message`、`retryable` 和 `details`。可选字段必须显式输出为 `null`，不能因为空而省略；输出消费者必须忽略未来新增字段。输入中的未知字段拒绝，以防拼写错误静默丢失。持久化 Checkpoint 或 History JSON 无法解码时返回 `DATABASE_UNAVAILABLE`，不得转换为空数组或 `null`；`resume` 必须在创建新 Session 前完成 Checkpoint 解码。

所有成功的 Task mutation 在 `data.task` 中返回完整 Task，其 `version` 是 mutation 后的新版本。`VERSION_CONFLICT` 的 `details` 返回 `expectedVersion` 和 `currentVersion`。Git 已经改变现场，或操作期间无法证明 Git 与数据库一致时，返回 `PARTIAL_EXTERNAL_STATE`；`details` 返回规范化 Repository/Worktree 路径、实际或 `unknown` Git 状态、稳定的 `databaseState`（`unchanged`、`updated` 或 `unknown`），以及可执行的 `adopt`、`detach` 或 `taskctl doctor` 建议。为保持 `schemaVersion: 1` 兼容，`recommendedCommand` 保持字符串；新增的 `recommendedArgs` 是不经过 Shell 拼接的参数数组，首项为 `taskctl`，并显式携带 `--database` 和规范化数据库路径；Task ID、Repository 与 Worktree 路径各自占用独立数组元素。

`gitState` 只能承载可判定的现场事实：无法证明现场时（例如后置观察失败）必须稳定输出字符串 `"unknown"`，不得用包含 `phase`/`observationError` 的对象冒充状态；`phase`、`observationError`、`observed` 等过程诊断统一放入可选 `diagnostics` 对象，无诊断时显式输出 `null`。`detach` 建议只在该 Task 的文件系统路径与 Git 登记均已证明不存在时给出；目录缺失但 Git 仍登记该 Worktree 时不得建议 `detach`（它必然被拒绝），应建议 `taskctl doctor` 并在 `diagnostics` 或 doctor issue 中说明需要先清理 Git 登记或恢复目录。

`databaseState=unchanged` 表示本次命令未提交数据库 mutation，`updated` 表示 mutation 已提交，`unknown` 表示提交结果无法确认；该字段不推断其他进程是否同时修改了数据库。

V0 稳定错误码固定为：

| code | 退出码 | retryable | `details` 最低字段 |
| --- | ---: | --- | --- |
| `INVALID_INPUT` | 2 | false | `field`、`reason` |
| `NOT_FOUND` | 2 | false | `entityType`、`id` |
| `UNSUPPORTED_SCHEMA_VERSION` | 2 | false | `databaseVersion`、`maxSupportedVersion` |
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
| `INTERNAL` | 10 | false | `diagnosticId` |

`retryable=true` 只表示重新读取状态或等待后重试可能成功，不授权自动覆盖、接管或执行 destructive 操作。V0 稳定警告码至少包括 `SENSITIVE_CONTENT_CHECK_REQUIRED`、`DUPLICATE_SESSION_IMPORT`、`PHYSICAL_ERASURE_NOT_GUARANTEED`、`RECORD_PATH_MISSING` 和 `INSECURE_DATABASE_PERMISSIONS`；Warning 的 details 也必须使用机器字段，不能只返回自然语言。

V0 DTO 固定如下；这里列出的可选字段也必须以 `null` 输出：

| DTO | 字段 |
| --- | --- |
| `TaskView` | `id`、`title`、`status`、`version`、`goal`、`scope`、`acceptanceCriteria`、`nextStep`、`blockReason`、`blockRecovery`、`currentSessionId`、`repositoryPath`、`repositoryCommonDir`、`repositoryBranch`、`worktreePath`、`latestCheckpointId`、`closureOutcome`、`closureReason`、`closedAt`、`createdAt`、`updatedAt` |
| `SessionView` | `id`、`taskId`、`source`、`externalSessionId`、`continuedFrom`、`recordPath`、`startedAt`、`endedAt` |
| `CheckpointView` | `id`、`taskId`、`sessionId`、`summary`、`completed`、`decisions`、`pending`、`nextStep`、`risks`、`gitHead`、`createdAt` |
| `TaskNoteView` | `id`、`taskId`、`sessionId`、`noteType`、`text`、`createdAt` |
| `SessionImportView` | `id`、`sessionId`、`sourcePath`、`mediaType`、`sha256`、`sizeBytes`、`importedAt`；不回传 BLOB 内容 |
| `WorktreeStatus` | `registered`、`repositoryPath`、`repositoryCommonDir`、`path`、`exists`、`branch`、`head`、`staged`、`unstaged`、`untracked`、`ignored`、`observedAt` |
| `HistoryEntry` | `id`、`taskId`、`sequence`、`changeType`、`sessionId`、`occurredAt`、`summary`、`payload` |

`WorktreeStatus.registered/exists` 是布尔值。`repositoryPath`、`repositoryCommonDir`、`path` 和 `branch` 是数据库登记值：`registered=true` 时必须保留并返回，即使文件系统中的 Worktree 已经不存在；`registered=false` 时这些字段为 `null`，`exists=false`。`exists` 和 `observedAt` 来自实时观察；`head`、`staged`、`unstaged`、`untracked`、`ignored` 也是观察值，Worktree 不存在时分别为 `null`，不能伪造空 HEAD 或空数组。Worktree 存在时四个文件数组按 Repository 相对路径字典序排列。只读 `worktree status` 的 Git 观察失败时返回 `GIT_COMMAND_FAILED`；Git mutation 启动后的后置观察无法证明现场未改变时返回 `PARTIAL_EXTERNAL_STATE`，二者都不能返回伪造状态。

命令的 `data` 映射固定为：

- `task show` 以及 `task create/claim/update/block/unblock/close`：`{ "task": TaskView }`；
- `task note`：`{ "task": TaskView, "note": TaskNoteView }`；
- `task checkpoint`：`{ "task": TaskView, "checkpoint": CheckpointView }`；
- `task list`：`{ "tasks": TaskView[] }`；V0 不分页，排序固定为 `updatedAt DESC, id ASC`；
- `task resume`：`{ "task": TaskView, "checkpoint": CheckpointView|null, "sessions": SessionView[], "worktreeStatus": WorktreeStatus|null, "nextStep": string|null }`；`sessions` 按 `startedAt ASC, id ASC`；
- `session show/list`：分别为 `{ "session": SessionView }` 和 `{ "sessions": SessionView[] }`；`session list` 按 `startedAt ASC, id ASC`；
- `session attach/close`：`{ "task": TaskView, "session": SessionView }`；
- `session import add/remove`：`{ "task": TaskView, "import": SessionImportView }`，remove 返回删除前的元数据快照且不含 BLOB；
- `session import list`：`{ "imports": SessionImportView[] }`，按 `importedAt ASC, id ASC`；
- `worktree status`：`{ "worktreeStatus": WorktreeStatus }`；
- `worktree create/remove/adopt/detach`：`{ "task": TaskView, "worktreeStatus": WorktreeStatus }`；
- `history`：`{ "history": HistoryEntry[] }`，按 sequence 升序；
- `doctor`：`{ "checks": [{ "code": string, "status": "ok"|"warning"|"error", "details": object }] }`。

DTO 与 SQLite 字段分离，但字段含义必须一一映射；时间统一输出 UTC RFC 3339，Task/Session/Checkpoint/Import ID 和 SHA-256 输出字符串，Note/History 自增 ID 输出 JSON 整数。任何破坏兼容性的字段删除、改名或语义改变必须增加 `schemaVersion`；只新增字段时消费者仍必须能够忽略。

## 8. 退出码

- `0`：成功；
- `2`：输入或 Schema 错误；
- `4`：版本、Session、数据库繁忙、Worktree 操作占用或约束冲突；
- `5`：Worktree 安全检查、路径身份检查或已证明未改变现场的 Git 执行失败；
- `6`：Git 已改变现场，或操作期间无法证明 Git 与数据库一致，返回 `PARTIAL_EXTERNAL_STATE`；
- `10`：数据库不可用或内部错误。
