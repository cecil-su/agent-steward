# CLI 与 MCP 设计

> 本文命令和工具名是 V1 候选契约，编码前仍可调整。

阶段 A/B 的 CLI 覆盖 Task/Review/TaskCheckpoint 与按需代码上下文；阶段 C 将同一核心接入现有 AI 会话。Assignment、Agent 控制和宿主 ContextWindow 命令属于可选阶段 D；Git 写入、知识与优化另行启用。本文 taskd 是可信应用核心的简称，不强制常驻服务。

首条路径为 `task create`（省略 --workspace 时解析或建立默认 Workspace）、记录下一步、`task checkpoint create`、`task context`、Review。写入目标有多个候选时要求显式选择，不按最近时间猜测。

## 1. 设计原则

- CLI 与 MCP 共用 Application Service、状态机和 Policy Engine。
- CLI 不通过修改文件实现任务操作。
- MCP 不通过 shell-out CLI 并解析文本实现业务逻辑。
- 人类和 AI 可以使用同一 CLI，但权限来自 connection principal/capability。
- MCP 工具隐藏是 UX，不是安全边界。
- 所有输出提供稳定 JSON 模式和关联 ID。

### 通用 Command Envelope

所有改变状态的 CLI/MCP Command 共用以下字段；CLI 使用 kebab-case，MCP 使用对应 camelCase：

- `--idempotency-key <key>`：所有写命令必填；taskd 以 principal + command type + key 定位 CommandReceipt，并绑定服务端计算的 requestHash。
- `--expected-version <n>`：更新单个既有可变 aggregate 时必填；纯创建新 aggregate 时不使用；若创建依赖既有 Task 等对象的状态，仍须提供该对象的前置版本。
- `--correlation-id <id>`：可选；省略时由 taskd 生成，客户端不能借此改变身份或授权。
- 跨 aggregate Command 不使用含义模糊的单一 expectedVersion，必须按对象命名所有前置版本，例如 expectedFactVersion 和 expectedCandidateRevisionVersion。
- requestHash 不由普通客户端声明；taskd 对 command schema version、target、payload、expectedVersions 和服务端绑定 scope 做 canonicalization 后计算，并在响应中返回。

principal、actingActor、session/assignment scope、causationId 和服务端 event metadata 不属于可覆盖参数。以下示例对并发敏感的核心写命令显式列出 Envelope；为保持命令族简洁而使用 `create|list|show` 等合并写法时，实际写子命令仍必须在 help、JSON schema 和服务端校验中强制适用字段。

### 只读诊断（阶段 A–C，候选入口）

`stewardctl doctor [--json]` 复用已授权 Query，返回问题类别、观察时间、当前事实与下一步，不自动修复或建立诊断权威表。阶段 C 接入诊断区分能力覆盖、契约验证、当前环境与真实接续；详细规则见 [第 21 篇第 8–9 节](21-continuity-and-review-contracts.md)。未实现的能力显示未支持，无法观察的状态显示 unknown；不自动连接模型或更改宿主配置。

## 2. CLI 命令族

### 身份和会话（按接入能力启用）

阶段 A 使用本地 Human 身份，C 冻结最小 Task scope 连接；以下 Session/Grant 管理命令按需要启用，不是建立普通任务的前置步骤。

```bash
stewardctl session attach
stewardctl session whoami
stewardctl session list
stewardctl session show <session-id>
stewardctl role accept <grant-id>
```

`session attach` 不接受 `--grant <value>`。CLI 只能通过无回显交互 stdin、受保护管道、继承句柄或可信本地 UI 获得一次性 capability；不得从 argv、Prompt 或可继承环境变量读取。

### Workspace 与 Repository 上下文（阶段 A/B，按需使用）

```bash
stewardctl workspace create --root <path> --idempotency-key <key>
stewardctl workspace list|show <workspace-id>
stewardctl workspace rename|archive <workspace-id> --expected-version <n> --idempotency-key <key>
stewardctl repo discover --workspace <workspace-id>
stewardctl repo register --workspace <workspace-id> --path <path> --idempotency-key <key>
stewardctl repo list --workspace <workspace-id>
stewardctl repo show|rescan|unlink <repository-id> [--expected-version <n>] [--idempotency-key <key>]
stewardctl worktree list --repository <repository-id>
stewardctl worktree show|rescan <worktree-id>
```

`discover` 和只读 `rescan` 返回 canonical real path、remote identity、Branch、HEAD、dirty/detached/missing 状态及 identity conflict，不写 Git。register 必须拒绝同一 Workspace 中解析到相同 identity 的重复 Repo；unlink 只把 registry row 转为 unlinked 并写审计事件，不执行目录删除、移动、clean 或其他 Git 命令。

### 任务与 Review（阶段 A/B）

```bash
stewardctl task list [--status ...] [--view needs-input] [--json]
stewardctl task show <task-id-or-external-id>
stewardctl task confirmations <task-id>
stewardctl task create [--workspace <workspace-id>] [--repo <repository-id>] [--worktree <worktree-id>] --idempotency-key <key>
stewardctl task update <task-id> --input <task-patch.json> --expected-version <n> --idempotency-key <key>
stewardctl task comment <task-id> --input <record.json> --expected-task-version <n> --idempotency-key <key>
stewardctl task transition <task-id> --to <status> --expected-version <n> --idempotency-key <key>
stewardctl task block <task-id> --input <blocker.json> --expected-version <n> --idempotency-key <key>
stewardctl task unblock <task-id> --input <resolution.json> --expected-version <n> --idempotency-key <key>
stewardctl task assign <task-id> --actor <actor-id> --expected-version <n> --idempotency-key <key>
stewardctl review show <submission-id>
stewardctl review withdraw <submission-id> --reason <text> --expected-task-version <n> --expected-submission-version <n> --idempotency-key <key>
stewardctl review submit <task-id> --evidence <artifact-link-id>@<expected-link-version>... --expected-task-version <n> --idempotency-key <key>
stewardctl review accept <submission-id> --expected-task-version <n> --expected-submission-version <n> --idempotency-key <key>
stewardctl review request-changes <submission-id> --expected-task-version <n> --expected-submission-version <n> --idempotency-key <key>
stewardctl task audit [<task-id>]
```

`task update` 只更新 schema 允许的目标、下一步、优先级、描述等字段，不接受 status/owner/actingActor 覆盖；生命周期与分配使用专用命令。`task comment` 追加进度、决策、约束或操作说明及来源，用户确认通过受控用户入口表达，AI 不能自称已获确认。Comment 为独立追加记录，不递增 Task version；Review 中的业务编辑返回 REVIEW_LOCKED，先 withdraw 再修改。字段见第 21 篇，物理存储尚待 D-024。

通用 `task transition` 不允许直接进入 Blocked、从 Blocked 恢复或从 Review 直接退出，分别使用 block/unblock 与 Review 专用命令。进入 Review 或 Done 也不允许通过通用 transition：这两个入口分别只由 `review submit` 和 `review accept` 提供。`review submit` 校验每个 source Link 的 expectedVersion、active 状态、权限及 Artifact contentHash，并在一个事务中为 ReviewSubmission 创建独立 active `review_evidence` Link、固定 submittedTaskVersion/acceptanceCriteriaHash/evidenceSetHash（含正文与 descriptor hash）、创建新的 reviewCycle 并把 Task 推进 Review。`review accept` 同时比较 Task/Submission 版本、已固定 hash，以及每个 submission-owned evidence Link 的 active 状态、版本、contentHash 和 descriptorHash；成功时在一个事务创建 accepted ReviewDecision、结束 Submission 并推进 Done。`review request-changes` 同样原子创建 Decision 并退回 In Progress。Done 重新打开固定回到 In Progress，旧 submissionId 只能读取，下一次 submit 必须创建新的 reviewCycle。

`review show` 返回第 21 篇的派生交付摘要；固定的提交事实和后来 Git 观察分列，不改变 Task。withdraw 由当前 Owner 或有管理权限的用户调用，校验当前版本和唯一 pending Submission，原子退回 In Progress，保存 reason 与审计；不要求旧 submittedTaskVersion 等于当前 Task version。accept、request-changes 与 withdraw 竞争时最多一个成功。

blocker 必须包含 reason、责任人或角色、requiredInput、resumeCondition；unblock 提交 resolution，不自动审批用户决定。needs-input 只读派生 Blocked/Review，不创建 Attention 状态。

### 证据登记（阶段 A）

```bash
stewardctl artifact attach <task-id> --file <path> [--descriptor <descriptor.json>] --expected-task-version <n> --idempotency-key <key>
stewardctl artifact read --link <artifact-link-id>
```

attach 在 ingest 时复制内容并生成稳定 Artifact/Link，不能只保存会变化的本地路径。descriptor 可提交 assertion、kind、报告结果与引用，origin/producer/hash 由核心派生；Agent 不得提交可信来源覆盖字段。只有已有可信采集入口的结果可被标为 system_observation/tool_result；本命令不执行报告中记载的命令。返回 artifactId/linkId/version/contentHash/descriptorHash，供 Review 固定。正文与 descriptor 一起遵守 publish-before-reference；写入 Task 证据关系会更新 Task version，因此 Review 期间先 withdraw。

### TaskCheckpoint 与恢复上下文（阶段 A；阶段 C 复用）

```bash
stewardctl task context <task-id> [--max-bytes <n>]
stewardctl task checkpoint create <task-id> --input <checkpoint.json> --expected-task-version <n> --idempotency-key <key>
stewardctl task checkpoint list <task-id>
stewardctl task checkpoint show <checkpoint-id>
```

context 返回当前目标、状态、owner、下一步、约束与决策、最近 Checkpoint、证据引用，以及带观察时间的 Git 状态。发现过期或缺失内容时显式返回差异，不声称 SQLite 与 Git 是同一原子快照。

context 的预算、必需事实、truncated/omittedSections/continuationRefs 见第 21 篇；这只限制本工具响应，不估算宿主完整窗口。核心事实放不下时返回 CONTEXT_BUDGET_TOO_SMALL。Checkpoint 追加不递增 Task version，保存失败必须返回明确错误。

Checkpoint schema 见第 05/21 篇与 D-024；不要求 sessionId、Assignment 或 Runtime。AI 使用服务端绑定 Task scope 调用同一命令；可选外部会话引用只作 provenance。

### Assignment/Agent（可选阶段 D）

```bash
stewardctl assignment create <task-id> --role writer --runtime herdr --idempotency-key <key>
stewardctl assignment complete <assignment-id> --report <artifact-id> --expected-version <n> --idempotency-key <key>
stewardctl agent spawn <assignment-id> --idempotency-key <key>
stewardctl agent resume <assignment-id> --session <session-id> --idempotency-key <key>
stewardctl agent send-prompt <assignment-id> --prompt-artifact <artifact-id> --expected-agent-run-version <n> --idempotency-key <key>
stewardctl agent status <assignment-id>
stewardctl agent close <assignment-id> --expected-agent-run-version <n> --idempotency-key <key>
stewardctl agent operation show <operation-id>
stewardctl agent operation reconcile <operation-id> --expected-version <n> --idempotency-key <key>
```

Task ownership 面向 Actor，active TaskOwnership 是唯一 Owner 权威；Session 只在 Assignment/Agent Run 的 spawn、resume 和 attach 流程中绑定，不能代替 active ownership。

`task assign --expected-version` 校验 Task.currentVersion；成功事务结束旧 active TaskOwnership、插入 ownerEpoch + 1 的新 row、递增 Task version 并写 TaskEvent。Task 表没有 ownerActorId 缓存列。

V1 每个 Assignment 最多一个 `starting/active` AgentRun，并由数据库 partial unique constraint 保证。`agent status` 返回该 agentRunId、版本和 runtimeHandleRef；send-prompt/close 由 taskd 在事务中解析并校验唯一 active AgentRun 及其 expected version，再把解析后的 agentRunId/runtimeHandleRef 纳入 canonical request 和 RuntimeOperation ordering scope。不存在 active Run 时返回 `NO_ACTIVE_AGENT_RUN`，不能按历史 Run 的时间戳猜测；数据库若检测到多个 active row 则视为完整性错误并拒绝调用 Runtime。

spawn/resume/send-prompt/close 返回 taskd 生成的稳定 operationId。重试相同 idempotency key/requestHash 返回同一 RuntimeOperation；结果未知时只能 show/reconcile。同一 Assignment/RuntimeHandle ordering scope 有未决 operation 时，更换 key 也返回 `RUNTIME_OPERATION_IN_DOUBT`，不能创建第二次冲突副作用。

`assignment complete --report` 只接受 finalized Artifact，并在完成 Assignment 的 SQLite transaction 中创建 `relationType=report` 的 active ArtifactLink；新报告必须先走 Artifact publish 协议。Artifact 本身不保存 assignmentId 或 taskId。

### Context Window、History 与 WorkingNote（可选阶段 D）

```bash
stewardctl context-window list --session <session-id>
stewardctl context-window show <context-window-id>
stewardctl context-window transition <context-window-id> --strategy <mode> --expected-version <n> --idempotency-key <key>
stewardctl history list-items --context-window <id> [--cursor ...]
stewardctl history read <history-item-id>
stewardctl history search --query <text> [--cursor ...]
stewardctl working-note list --assignment <assignment-id>
stewardctl working-note read <note-id>
stewardctl working-note create --assignment <assignment-id> --logical-name <name> --idempotency-key <key>
stewardctl working-note append <note-id> --expected-version <n> --expected-content-link-version <n> --idempotency-key <key>
stewardctl working-note write <note-id> --expected-version <n> --expected-content-link-version <n> --idempotency-key <key>
```

Human/Admin 可以在授权范围内显式选择 Session/Assignment。Agent connection 的 sessionId、assignmentId 和 principal 由 taskd 绑定；命令中的过滤条件只能缩小 scope，不能扩大或替换 connection scope。

### Git（按需 inspect；写入另行启用）

```bash
stewardctl git inspect <task-id>
stewardctl git plan commit|merge|push <task-id>
stewardctl git approve <plan-id>
stewardctl git execute <plan-id>
stewardctl git verify <operation-id>
```

阶段 A 的 Repo/Worktree 身份与状态读取不经过 Git 写入命令族。`git inspect` 按既有 TaskContextBinding 使用；plan/approve/execute 属于另行启用的 Git 写入能力。`approve` 只有在可信 Human principal 下才可用；Agent shell 中相同命令必须被服务端拒绝。

### 数据（阶段 A 起）与优化（探索 X）

backup/export/inspect 随实际存储启用；optimize 命令仅是探索 X 的候选接口，不进入主线 CLI 验收。

```bash
stewardctl data backup create --destination <path> --idempotency-key <key>
stewardctl data backup verify <backup-path>
stewardctl data backup restore <backup-path> --idempotency-key <key>
stewardctl data export|inspect|delete-session|retention
stewardctl optimize observe
stewardctl optimize propose
stewardctl optimize experiment
stewardctl optimize promote <proposal-id>
```

首期 backup create 使用显式维护停写模式，覆盖 Task/Checkpoint/上下文与实际使用的证据；启用加密 Blob 后覆盖 Blob/key envelope。以下 gate/pin/operation 规则适用于在线扩展，恢复仍需 D-021 的 generation 契约。SQLite 一致性点由覆盖全部 writer 的数据库层 gate 固定；发布后响应丢失时按 final manifest reconcile 同一 BackupOperation。backup restore 要求 taskd 独占维护模式，在隔离临时根验证并 normalization 后原子切换；幂等键、requestHash、manifest hash 和 old/new data-root generation 记录在被替换根之外的 bootstrap journal。相同请求在切换后重试只能继续/返回原 operation，不能再次 restore。

### 业务事实与实现认知（探索 X）

```bash
stewardctl fact propose|show|history
stewardctl fact confirm <revision-id> --expected-fact-version <n> --expected-candidate-version <n> --expected-current-revision <id|none> [--expected-current-revision-version <n>] --idempotency-key <key>
stewardctl fact reject <revision-id> --expected-revision-version <n> --idempotency-key <key>
stewardctl snapshot capture-commit|capture-worktree|show
stewardctl mapping propose --fact <fact-id> --reference <implementation-reference-id> --idempotency-key <key>
stewardctl mapping confirm|reject <mapping-id> --expected-version <n> --idempotency-key <key>
stewardctl mapping show <id> --context-type <type> --context-key <key>
stewardctl mapping observation create --mapping <id> --snapshot <id> --context-type <type> --context-key <key> --expected-context-revision <rev> --state current|stale|missing [--observed-reference-revision <id>] [--expected-reference-revision <id>] --idempotency-key <key>
stewardctl drift list|show|classify|resolve|dismiss|create-task
```

`confirm`、`reject`、`classify`、`resolve` 和 `dismiss` 根据 policy 要求 Human 或授权 Reviewer；AI Scanner 默认只能 capture snapshot、propose mapping/fact revision 和创建 DriftFinding。

FactRevision confirm 在一个事务中比较 BusinessFact version、candidate revision version、expectedCurrentRevisionId，以及存在旧 confirmed revision 时的 version；任一前置条件变化都返回冲突，不允许后写覆盖先写。Mapping V1 只接受 factId；observation context 由 taskd 校验并规范化，客户端不能把任意 snapshot 冒充当前 Branch/Worktree/baseline revision。current/stale observation 要求 observed-reference-revision；missing 禁止该字段并要求 expected-reference-revision。

## 3. MCP 工具候选

阶段 C 可先以接续 CLI Skill 验证，再按需交付 MCP。MCP 子集为 Task 查询、task_update/task_comment_create、task_block/task_unblock、task_context、task_checkpoint_create/list/get、artifact_attach/read 与 review_submit/show；当前 Owner 有权限时可 review_withdraw。review_accept/request-changes 的 AI Reviewer capability、Assignment/Agent/ContextWindow 工具均为后续能力，不能据下面完整候选表推断为阶段 C 必交付。阶段 C 的验收由用户入口执行。


### 只读

```text
session_whoami
workspace_get
repository_list
repository_get
worktree_list
worktree_get
task_list
task_get
task_get_confirmations
task_context
task_checkpoint_list
task_checkpoint_get
review_submission_get
review_show
artifact_read
task_get_next
task_audit
event_inspect
agent_get_status
runtime_operation_get
git_inspect
optimization_get_proposals
context_window_list
context_window_get
history_item_list
history_item_read
history_search
working_note_list
working_note_read
business_fact_get
business_fact_history
implementation_snapshot_get
implementation_mapping_get
drift_finding_list
drift_finding_get
```

### 受控写入

```text
task_create
task_update
task_comment_create
task_block
task_unblock
artifact_attach
task_checkpoint_create
task_transition
task_assign_owner
review_submit
review_withdraw
review_accept
review_request_changes
assignment_create
assignment_complete
agent_spawn
agent_resume
agent_send_prompt
agent_close
runtime_operation_reconcile
event_ack
git_plan_create
git_plan_execute
optimization_propose
context_window_transition
working_note_create
working_note_append
working_note_write
business_fact_revision_propose
implementation_mapping_propose
mapping_observation_create
drift_finding_create
```

`review_accept` / `review_request_changes` 只对具有当前 Task scope Reviewer capability 的 connection 开放；AI Worker grant 不包含该能力。业务事实确认、mapping 确认和 DriftFinding 结论不作为普通 AI MCP 工具暴露；后续如需开放，必须使用独立 Reviewer grant 和服务端 policy，而不能由 tool arguments 自称 Reviewer。

History、WorkingNote 与 Mapping MCP 契约要求：

- taskd 从 connection 注入 principalId、sessionId、assignmentId，普通参数不能覆盖；
- list/search 使用稳定 cursor 和受服务端限制的 page size；snapshot 与 eventWatermark 必须来自同一 SQLite read transaction，Event Stream 只接受 `streamPosition > watermark`，过期 cursor 返回 CURSOR_EXPIRED；
- read/search 返回 `truncated`、原始大小、返回大小和 continuation 信息；
- History 只读，任何删除只能通过受审计的数据保留接口；
- WorkingNote create 必须带 logicalName 和 idempotency key，并保证同一 Assignment 内 active logicalName 唯一；append/write 必须带 Note expectedVersion、current content Link expectedVersion 和 idempotency key；
- implementation_mapping_get 与 mapping_observation_create 必须携带 observation context；taskd 校验 contextKey/contextRevision 与当前 baseline、ref 或 worktree 一致，强制 observed/expected Revision 属于 Mapping 的同一 ImplementationReference、observed snapshot 与 Observation 完全一致、expected baseline/last-known context 兼容，以及 current/stale/missing 的空值不变量；查询没有匹配 observation 时返回 `unobserved`；
- context transition 必须匹配 Runtime capability，并记录统一 DomainEvent。

不提供：

- 任意 SQL；
- 任意文件写入；
- 任意 Git 命令；
- 由调用者指定“我是 registry-main”；
- 不带 expectedVersion 的覆盖式更新。

## 4. MCP Connection 绑定

阶段 C 由可信入口绑定 principalId、actingActor、Task scope 和适用 capability；可选外部 session reference 只作来源。阶段 D 启用受管 Runtime 后才额外绑定 sessionId、assignmentId、invocationId 与 runtime identity。普通工具参数不得覆盖这些事实。

接续 CLI Skill 只指导读取、写入与保存 Checkpoint，不签发权限或扩大 scope；交付与失败验收见第 21 篇。Skill 版本跟随 CLI JSON/schema，不另写状态规则。

## 5. 错误契约

错误应结构化，例如：

```json
{
  "ok": false,
  "error": {
    "code": "STALE_OWNER_EPOCH",
    "message": "Task owner grant is no longer current",
    "currentVersion": 18,
    "recovery": "Refresh task facts and request a new grant"
  }
}
```

关键错误类别：

- UNAUTHENTICATED
- FORBIDDEN_SCOPE
- STALE_VERSION
- CURSOR_EXPIRED
- IDEMPOTENCY_KEY_REUSED
- STALE_OWNER_EPOCH
- LEASE_CONFLICT
- REVIEW_LOCKED
- CONTEXT_BUDGET_TOO_SMALL
- INVALID_TRANSITION
- PLAN_EXPIRED
- GIT_FACT_CHANGED
- APPROVAL_REQUIRED
- RUNTIME_CAPABILITY_UNAVAILABLE
- RUNTIME_OPERATION_IN_DOUBT
- NO_ACTIVE_AGENT_RUN
- NEEDS_RECONCILIATION

## 6. 幂等

所有改变状态的 Command，包括 Runtime 外部副作用、创建 Assignment、完成事件、ack、Context transition、WorkingNote create/append/write 和 Git execute，都必须支持并要求 idempotency key。taskd 持久化 `(principalId, commandType, idempotencyKey, requestHash, status, resultRef)`：相同 key + 相同 requestHash 不重复执行，但返回历史 payload/resultRef 前仍需校验当前 connection、grant 和对象读取权限；无权时返回 FORBIDDEN_SCOPE 或脱敏终态。相同 key + 不同 requestHash 返回 `IDEMPOTENCY_KEY_REUSED`，不得执行新请求、覆盖旧 receipt 或产生 DomainEvent。幂等是副作用保证，不是授权缓存。
