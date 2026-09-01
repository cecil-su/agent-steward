# V1 需求说明

## 1. 优先级 A：Workspace 与 Repository 基础

### FR-001 Workspace

- 创建、打开、重命名和归档 Workspace；Workspace 是 Repository、Worktree、Task 和后续 BusinessFact 的必填本地边界。
- 首期不要求用户创建 Project，也不允许 Repository 或 Task 在 Workspace 之外成为无归属对象。
- Workspace unlink/archive 只改变产品内关联和可见性，不删除用户目录、Git 数据或未跟踪文件。

### FR-002 Repository 与 Worktree Registry

- 自动发现或手动注册 Repository，保存 canonical real path、remote identity、provider、availabilityState 和版本。
- 在 Repository 下发现 Worktree，保存 canonical real path、Branch/HEAD、detached/dirty/missing 等只读状态。
- path/remote identity 重复、大小写、符号链接、移动和暂时不可用必须有确定性处理，不能按字符串路径静默创建重复 identity。
- rescan、refresh 和 unlink 不得执行 checkout、clean、reset、commit、merge、push，不得删除或移动用户文件。

### FR-003 Registry 视图与客户端

- 至少一个首期客户端能完成 Workspace onboarding、Repo 发现/确认、Worktree 状态查看、rescan 和 unlink；TUI/GUI 共享同一 Command/Query/Event 契约。
- Workspace 概览能区分 available、missing、moved/identity conflict 和 detached/dirty Worktree，并说明安全恢复动作。
- Snapshot Query 与 eventWatermark 来自同一 SQLite read transaction；cursor 超出 retention 时强制重新获取 snapshot。

### FR-004 Registry 本地数据与恢复

- SQLite 是 Registry 结构化状态的唯一权威，核心操作离线可用且默认不上传遥测。
- metadata backup/restore 使用覆盖所有 SQLite writer 的 gate、完整性 manifest、启动 reconcile 和 data root 外 bootstrap journal；restore 原子切换 generation。
- 重启或恢复后 Workspace/Repository/Worktree identity 保持稳定；解除关联不被恢复器误解释为文件系统删除任务。

## 2. 优先级 B：任务与 Review 闭环

### FR-101 Task、Subtask 与上下文绑定

- 创建、编辑、归档 Task 和 Subtask；Task.workspaceId 必填。
- Task 至少包含标题、描述、状态、优先级、owner、下一步和验收标准，不包含必填 projectId。
- TaskContextBinding 可把 Task 关联零个或多个同一 Workspace 内的 Repository/Worktree，并区分 primary/related；Task 也允许只属于 Workspace 而不绑定具体 Repo。
- 支持父子、阻塞和相关关系；关系必须可查询、可视化且避免明显循环依赖。

### FR-102 生命周期

- 默认状态为 `Inbox → Ready → In Progress → Blocked → Review → Done`。
- `Cancelled` 是业务终态；归档通过正交的 `archiveState=archived` 表达，不覆盖 Done、Cancelled 等生命周期状态。
- 状态转换由统一状态机校验，并记录操作者、原因、时间和版本。
- 从 Blocked 恢复时回到明确的可执行状态，而不是丢失此前上下文。
- 每次进入 Review 创建绑定 submittedTaskVersion、acceptanceCriteriaHash 和版本化 evidence set 的 ReviewSubmission；submit 必须为每个证据创建由 ReviewSubmission 独立持有的 active `review_evidence` ArtifactLink，证据权限、保留和删除不得继续依赖 source Link。
- accept/request-changes 必须同时校验 Task 与 Submission 版本；accept 还必须验证全部 submission-owned evidence Link 仍 active、版本及 contentHash 匹配，并在一个事务中写 ReviewDecision 和状态转换。
- Done 重新打开固定回到 In Progress；下一次验收必须创建新的 reviewCycle，历史 accepted Decision 不得被复用。

### FR-103 任务管理与所有权

- Task Manager 能分流 Inbox、设定优先级、建立依赖、分配 owner、跟踪阻塞并选择下一任务。
- 每个活跃任务最多有一个当前 Task Owner；Worker 可以有多个。
- active TaskOwnership 是 Owner 唯一权威；Task 不保存重复 ownerActorId，数据库必须阻止同一 Task 出现两个 active ownership。
- Owner 可以是人、AI 或自动化 Actor，但都受相同生命周期约束。
- Task Owner 必须维护唯一下一步；没有下一步的活跃任务应被系统提示。

### FR-104 视图、查询与提醒

- 提供 Workspace Inbox、Today/Next、按 Repository/Worktree、按 owner、Blocked、Review 和 Done 视图。
- 支持搜索、过滤、排序、保存视图和查看任务时间线。
- 能识别无 owner、无下一步、长期阻塞和等待验收的任务。

### FR-105 TUI 与 GUI

- TUI 与 GUI 均能完成捕获、查看、编辑、推进、阻塞、验收和归档。
- 两端共享 Command、Query 与 Event 契约，不直接各自实现业务规则。
- 任一客户端写入后，另一客户端能通过事件流增量刷新；断线恢复必须在同一 SQLite read transaction 获得 snapshot + eventWatermark，并只消费更大的全局 streamPosition。cursor 超出 retention 时强制重新获取 snapshot。

### FR-106 事件、评论与产物

- 所有 aggregate 状态变化生成带必填 schemaVersion 的不可变 DomainEvent，支持幂等写入和按 aggregate/version 读取。
- DomainEvent 使用与 aggregateVersion 分离的全局单调 streamPosition；内部 Artifact storage audit 不进入普通客户端的业务 Event Stream。
- TaskEvent 是 `aggregateType=task` 的领域事件族；后续 FactRevision、Mapping、Snapshot 和 DriftFinding 使用相同 event envelope。
- Artifact 是不强制绑定 Task 的通用内容对象；Task、Assignment、FactRevision、Snapshot、WorkingNote 和 Wiki 通过 ArtifactLink 声明用途与归属。
- 支持评论、附件、链接、交付物和验收记录；同一 Artifact 可以被多个业务对象引用，而不复制正文。
- Blob 写入采用 pending metadata、durable finalize intent、同文件系统原子发布、finalize transaction、启动恢复与孤儿 GC 协议；Intent 必须保存规范化请求、requestHash、目标、完整 expectedVersions 和 Link 切换计划，业务冲突成为终态而非永久重试。
- ArtifactLink 具有 active/superseded/unlinked 状态和版本；正文切换必须原子更新 currentContentLinkId，并保留可审计的旧 Link。
- V1 一个 Artifact 独占一个物理 Blob；GC 必须先以 deleting 状态原子 claim，Link 创建只允许 finalized Artifact，避免删除与新 Link 的竞态。
- 任务当前状态由事务化数据维护；事件历史用于时间线、同步、恢复和审计。

### FR-107 Task Artifact 与恢复扩展

- SQLite 是结构化状态的唯一权威；Markdown 只作为导出格式。
- 默认不上传遥测，离线时仍能完成全部任务管理操作；沿用 Phase 1 的 data root、writer gate 和 restore journal。
- 支持备份、恢复、数据导出、删除和 schema migration。备份必须从单一 SQLite 一致性点生成 Artifact manifest/eventWatermark，以 active backup pin 阻止 GC，并在发布前验证数据库、全部 Blob 和可移植加密 key envelope；restore 必须先在隔离临时根验证 Link → Blob 完整性，再原子启用整套数据。

## 3. 优先级 C：AI 执行与流程体验

### FR-201 AI 接入

- AI 通过通用 Actor 身份成为 Owner 或 Worker，不创建独立任务副本。
- MCP、CLI 和 Runtime Adapter 调用与 TUI/GUI 相同的 Application Service。
- 首期只需支持一个 AI Runtime 的端到端垂直切片，其余通过 Adapter contract 后续扩展。

### FR-202 Assignment 与子代理

- Task Owner 可把阶段工作拆成 Assignment 并交给一个或多个 Agent Run。
- 支持记录 Session、Invocation、上下文摘要、允许操作和产物。
- WorkingNote 支持按 Assignment 和 logicalName 幂等创建，再通过 Note 与 current content Link 的 expectedVersion 追加或替换正文。
- spawn、resume、sendPrompt 等 Runtime 副作用使用持久化 RuntimeOperation 和稳定 operationId；响应丢失或 taskd 崩溃后必须 reconcile，无法判定时进入 needs_reconciliation，不得盲目重试。
- 同一 Assignment/RuntimeHandle ordering scope 存在未决 operation 时阻止后续冲突副作用，更换 idempotency key 不能绕过。
- V1 每个 Assignment 最多一个 starting/active AgentRun；sendPrompt/status/close 必须解析该唯一 active Run，不得按最新时间猜测 RuntimeHandle。
- Runtime 的 `completed` 只把任务或 Assignment 推进到 Review，不能直接写 Done。

### FR-203 证据、权限与高风险操作

- AI 输出必须关联 Artifact、测试结果或可复核说明。
- 高风险 Git 或系统操作采用 Plan → Approve → Execute → Verify。
- Agent 不得自我提权、伪造用户批准或删除任务和审计历史。

## 4. 优先级 D：业务事实、实现认知与优化

### FR-301 Workspace BusinessFact

- BusinessFact、BusinessSystem 和 Scenario 建立在 Phase 1 的 Workspace 上；Repository 继续承载实现，Task 通过 Workspace/TaskContextBinding 组织工作，不引入 Project 双重边界。
- 支持 Business System、Scenario、Requirement、Business Rule、Decision、Glossary 和 FactRevision。
- 业务事实使用稳定 ID，不以 Branch、文件路径、Symbol 或 Commit SHA 作为身份。
- FactRevision 使用 `candidate → confirmed/rejected`，新 confirmed revision 通过 `superseded` 保留旧版本，不原地覆盖历史。
- 确认 FactRevision 必须在一个事务中校验 BusinessFact、candidate revision 和当前 confirmed revision 的并发前置条件。
- 提供 propose、confirm、reject、supersede、查询版本和影响范围等 Command/Query。

### FR-302 实现快照、映射与漂移

- Repository、Component、Worktree 和 Implementation Reference 属于实现认知层，并归属 Workspace。
- `CommitSnapshot` 只表示确定 Commit 的提交树；`WorktreeSnapshot` 单独保存可复核的 canonical index manifest、staged/unstaged diff 和 untracked manifest Artifact。
- WorktreeSnapshot 必须表达 unmerged index stages、submodule 和 sparse checkout；无法采集敏感或超限内容时标记 partial 和 missing evidence，不能宣称为完整证据。
- WorktreeSnapshot 的完整多 Artifact 清单必须由 durable CaptureOperation 持有；全部 Blob 发布并比较采集前 token、由 evidence 派生的 token 和采集后 token 后，才能在一个 SQLite transaction 中 finalize 全部 Artifact、创建 Snapshot、全部 evidence Link 和计划内 Observation。任何 Blob、版本或 token 不一致都应整体失败，不能暴露半完成或以 partial 发布撕裂现场。
- V1 只允许 BusinessFact 到实现位置的 Mapping；Scenario 通过关联的 BusinessFact 间接获得实现视图。MappingObservation 必须绑定 CommitSnapshot 或 WorktreeSnapshot，不能把不同未提交现场归到同一个 commitSha。
- ImplementationReference 使用稳定身份，locator 由绑定 snapshot 的不可变 ImplementationReferenceRevision 表达；ImplementationMapping 保存稳定逻辑关系，current/stale/missing 由不可变 MappingObservation 表达。
- freshness read model 必须按 Workspace baseline、Repository + selected ref、Worktree 或 release baseline 等观察上下文派生；不得跨不相关 Branch/Worktree 按时间选择全局最新 observation。
- MappingObservation 的 observed/expected Revision 必须与 Mapping 属于同一 ImplementationReference；observed Revision 的 snapshot 必须与 Observation 完全一致。current/stale 必须引用实际观察到的 Revision；missing 不得伪造 observed revision，并必须引用与指定 baseline/last-known context 兼容的 expected Revision。
- DriftFinding 支持 open、classified、resolved 和 dismissed，并可分类为 branch exception、code issue 或 fact change。
- 提供创建快照、提出/确认映射、更新 freshness state、分类/解决 finding，以及从 finding 创建 Task 的 Command/Query。
- AI 扫描只能生成 candidate mapping、FactRevision 或 DriftFinding，不能直接改写 confirmed fact。

### FR-303 需求与偏好整理

- 从用户明确输入、任务决策和验收反馈中整理 BusinessFact 与 Preference；Requirement 和 Decision 是 BusinessFact 类型。
- 每条内容保留来源、作用域、置信度和确认状态。
- 外部网页、仓库文本和 AI 推断不能自动成为已确认的用户偏好。

### FR-304 优化候选

- 识别重复澄清、返工、阻塞模式、工具失败和上下文浪费。
- 生成流程、Prompt、模板或 Skill 候选，并附证据、预期收益、风险和回滚方法。
- 未经用户确认不得修改 live 配置、规则、Prompt、Skill 或代码。

## 5. 非功能需求

### NFR-001 一致性与可靠性

- aggregate 状态写入和对应 DomainEvent/outbox envelope 在同一事务提交。
- 可变实体使用 `currentVersion` / `expectedVersion` 防止静默覆盖。
- idempotency key 必须绑定服务端规范化请求的 requestHash；同一 key 携带不同语义请求时返回确定性冲突。
- 命中历史 CommandReceipt 只阻止重复副作用；返回 status/resultRef 前仍须校验当前 connection、grant 和对象读取权限。
- 文件系统 Blob 与 SQLite 使用 publish-before-reference 恢复协议，不宣称跨两者的 ACID；崩溃后不得留下业务可见的悬空 ArtifactLink。
- 进程崩溃后可恢复到已确认状态，不依赖聊天摘要猜测。

### NFR-002 可用性

- 高频操作在 TUI 中键盘可达，在 GUI 中可发现。
- 核心交互不依赖网络、AI 账号或外部服务。
- 错误信息必须说明失败原因和可恢复动作。

### NFR-003 可测试性与扩展性

- 状态机、依赖、版本冲突、事件同步和数据迁移具有确定性测试。
- TUI、GUI、CLI 与 MCP 共享契约测试。
- 核心不写死操作系统、Agent Runtime 或 Git 平台。

### NFR-004 隐私与安全

- 默认本地存储、无遥测；敏感附件和会话数据提供加密路径。
- 威胁模型防范受 Prompt Injection 影响、拥有普通 Workspace/Repo shell 权限的 Agent 越权。
- 不以抵御本机管理员、恶意软件或操作系统攻破为目标。

## 6. 分阶段验收

### `0.1` Workspace 与 Repository Registry

- 能创建 Workspace，发现/注册多个 Repository 和 Worktree，并稳定显示 path、remote identity、Branch、HEAD、dirty/detached/missing 状态。
- 同一 Repo 经符号链接、大小写差异或重复扫描不会静默产生第二个 identity；移动或不可用时给出确定状态。
- rescan/unlink 的故障测试证明不会修改 Git、删除目录或清理未跟踪文件。
- 至少一个客户端可完成 onboarding；第二客户端读取同一 snapshot/event contract 时不产生状态分叉。
- metadata backup writer gate 覆盖所有 SQLite 写路径；发布/切换/响应丢失故障可由 BackupOperation/RestoreBootstrapJournal 幂等收敛。

### `0.2` Task 与 Review

- Task 必须归属 Workspace，可绑定同 Workspace 的 Repo/Worktree，不需要 Project。
- 不配置 AI 也能完成捕获、分流、分配、推进、阻塞、Review、Done、reopen 和归档闭环。
- TUI 与 GUI 展示同一 Task 状态；子任务、依赖、owner、下一步、评论、产物和时间线可用。
- Artifact 崩溃注入后可恢复或 GC，不产生指向缺失 Blob 的 active Link。
- source Link 被 supersede/unlink 后 submission-owned evidence 仍可读取；evidence Link/version/hash 不匹配时 accept 失败，旧 Decision 不能在 reopen 后再次推进 Done。
- Artifact backup/restore 可验证 SQLite、manifest、Blob hash 和 key envelope，非终态外部 operation 不自动重放。

### `0.3` AI 执行

- AI 使用现有 Task 与生命周期，不形成第二套状态权威。
- AI 完成后必须经过 Review；失败、阻塞和恢复都有可追踪事件。
- 人类与 AI 通过不同客户端得到一致的权限和状态判定。
- WorkingNote 可按 Assignment + logicalName 幂等创建，并对并发 append/write 返回确定性版本冲突。
- Runtime 在外部副作用成功但本地结果未提交、响应丢失和重复请求场景下不会重复 spawn/resume/sendPrompt；无法自动核实时明确进入 needs_reconciliation。
- 并发 spawn/resume 不会为同一 Assignment 产生两个 starting/active AgentRun，prompt/control 命令始终命中唯一 Run。

### `0.4.0` BusinessFact 与 Workspace Knowledge

- 既有 Workspace、Business System、Scenario、Business Fact、Repository 和 TaskContextBinding 的关系可查询且没有双重权威。
- FactRevision 的候选、确认、拒绝、supersede 和历史浏览流程可验证；并发确认两个 candidate 时最多一个事务成功。
- 禁用代码扫描和 Optimizer 后，用户仍可维护业务事实并关联 Task。

### `0.4.1` 实现快照、映射与漂移

- CommitSnapshot 与 WorktreeSnapshot 能区分同一 HEAD 下的不同未提交现场，映射与 finding 可追溯到具体快照。
- WorktreeSnapshot 的 index/diff/untracked evidence 可长期复核，partial capture 被明确标记。
- 在任一 evidence Blob 发布失败，或 index、tracked file、untracked file 于采集中变化的故障测试中，CaptureOperation 只会重试/整体失败，不会发布半完成或由多个时刻拼成的 Snapshot/Observation。
- ImplementationReferenceRevision、MappingObservation 和 DriftFinding 的追加、分类、解决与历史保留流程可验证；跨 ImplementationReference、observed snapshot 不一致或 expected context 不兼容的 Observation 必须被拒绝。
- 扫描旧 Branch/实验 Worktree 的时间晚于 main 时，main/baseline freshness 仍只由自身观察上下文决定。

### `0.4.2` Preference 与 Optimizer

- 已确认需求和偏好与推断候选清晰分离。
- 优化候选可以追溯到任务证据，应用前需要用户确认，并可回滚。
- 能证明至少一类重复澄清、返工或流程浪费得到下降。
