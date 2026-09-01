# CLI 与 MCP 设计

> 本文命令和工具名是 V1 候选契约，编码前仍可调整。

阶段说明：`0.1` 先实现 Workspace/Repository/Worktree Registry 的 Command/Query/Event 子集；`0.2` 增加 Task/Review，MCP 与 AI 专用命令在 `0.3` 接入。

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
- `--expected-version <n>`：更新单个既有可变 aggregate 时必填；创建命令不使用。
- `--correlation-id <id>`：可选；省略时由 taskd 生成，客户端不能借此改变身份或授权。
- 跨 aggregate Command 不使用含义模糊的单一 expectedVersion，必须按对象命名所有前置版本，例如 expectedFactVersion 和 expectedCandidateRevisionVersion。
- requestHash 不由普通客户端声明；taskd 对 command schema version、target、payload、expectedVersions 和服务端绑定 scope 做 canonicalization 后计算，并在响应中返回。

principal、actingActor、session/assignment scope、causationId 和服务端 event metadata 不属于可覆盖参数。以下示例对并发敏感的核心写命令显式列出 Envelope；为保持命令族简洁而使用 `create|list|show` 等合并写法时，实际写子命令仍必须在 help、JSON schema 和服务端校验中强制适用字段。

## 2. CLI 命令族

### 身份和会话

```bash
stewardctl session attach
stewardctl session whoami
stewardctl session list
stewardctl session show <session-id>
stewardctl role accept <grant-id>
```

`session attach` 不接受 `--grant <value>`。CLI 只能通过无回显交互 stdin、受保护管道、继承句柄或可信本地 UI 获得一次性 capability；不得从 argv、Prompt 或可继承环境变量读取。

### Workspace 与 Repository Registry（`0.1`）

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

### 任务与 Review（`0.2`）

```bash
stewardctl task list [--status ...] [--json]
stewardctl task show <task-id-or-external-id>
stewardctl task confirmations <task-id>
stewardctl task create --workspace <workspace-id> [--repo <repository-id>] [--worktree <worktree-id>] --idempotency-key <key>
stewardctl task transition <task-id> --to <status> --expected-version <n> --idempotency-key <key>
stewardctl task assign <task-id> --actor <actor-id> --expected-version <n> --idempotency-key <key>
stewardctl review submit <task-id> --evidence <artifact-link-id>@<expected-link-version>... --expected-task-version <n> --idempotency-key <key>
stewardctl review accept <submission-id> --expected-task-version <n> --expected-submission-version <n> --idempotency-key <key>
stewardctl review request-changes <submission-id> --expected-task-version <n> --expected-submission-version <n> --idempotency-key <key>
stewardctl task audit [<task-id>]
```

通用 `task transition` 不允许直接进入 Review 或 Done：这两个入口分别只由 `review submit` 和 `review accept` 提供。`review submit` 校验每个 source Link 的 expectedVersion、active 状态、权限及 Artifact contentHash，并在一个事务中为 ReviewSubmission 创建独立 active `review_evidence` Link、固定 submittedTaskVersion/acceptanceCriteriaHash/evidenceSetHash、创建新的 reviewCycle 并把 Task 推进 Review。`review accept` 同时比较 Task/Submission 版本、已固定 hash，以及每个 submission-owned evidence Link 的 active 状态、版本和 contentHash；成功时在一个事务创建 accepted ReviewDecision、结束 Submission 并推进 Done。`review request-changes` 同样原子创建 Decision 并退回 In Progress。Done 重新打开固定回到 In Progress，旧 submissionId 只能读取，下一次 submit 必须创建新的 reviewCycle。

### Assignment/Agent（`0.3`）

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

### Context Window、History 与 WorkingNote（`0.3`）

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

### Git（`0.3` inspect；`0.5` write）

```bash
stewardctl git inspect <task-id>
stewardctl git plan commit|merge|push <task-id>
stewardctl git approve <plan-id>
stewardctl git execute <plan-id>
stewardctl git verify <operation-id>
```

Phase 1 的 Repo/Worktree discovery 不经过本命令族且只读。`git inspect` 最早在 AI 阶段按既有 TaskContextBinding 使用；plan/approve/execute 属于高级 Git 阶段。`approve` 只有在可信 Human principal 下才可用；Agent shell 中相同命令必须被服务端拒绝。

### 数据与优化

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

`0.1` 的 backup create 先覆盖 Registry metadata；`0.2` 起扩展 Artifact manifest/Blob/key envelope。SQLite 一致性点由覆盖全部 writer 的数据库层 gate 固定；发布后响应丢失时按 final manifest reconcile 同一 BackupOperation。backup restore 要求 taskd 独占维护模式，在隔离临时根验证并 normalization 后原子切换；幂等键、requestHash、manifest hash 和 old/new data-root generation 记录在被替换根之外的 bootstrap journal。相同请求在切换后重试只能继续/返回原 operation，不能再次 restore。

### 业务事实与实现认知（`0.4`）

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
review_submission_get
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
task_transition
task_assign_owner
review_submit
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

MCP Server 启动时由可信宿主或 taskd 绑定：

- principalId
- sessionId
- assignmentId
- invocationId
- grant/capability
- runtime identity

这些事实不能由普通 tool arguments 覆盖。

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
