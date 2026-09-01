# V1 实施路线

V1 采用可验证的垂直切片推进。唯一交付顺序是：先证明 Workspace/Repository Registry 的本地工作边界，再在该边界上证明任务与验收价值，然后接入 AI，最后增加长期业务事实、实现认知和优化能力。后续层不得倒逼前一层依赖尚未交付的能力。

## Phase 0：Workspace / Repository 基线与技术实验

- 冻结 Workspace、Repository、Worktree 的最小身份、归属、可用性状态和 unlink 语义；
- 选择技术栈、SQLite migration 方案、本地进程边界和 data-root generation 布局；
- 验证 Windows、macOS、Linux 的 canonical real path、符号链接、大小写、Git discovery 和重复注册规则；
- 确认首个 TUI/GUI onboarding 入口与共享 Command/Query/Event 契约；
- 验证事务写入 aggregate + DomainEvent/outbox、全局 streamPosition、同事务 snapshot + eventWatermark 和版本冲突；
- 定义覆盖全部 SQLite writer 的 backup gate、SQLite online backup、发布崩溃 reconcile，以及 data root 外 bootstrap journal 驱动的幂等 restore；
- 用低保真原型验证创建 Workspace、发现/确认 Repo、查看 Worktree 状态和解除关联四条高频路径。

退出条件：Workspace/Repo/Worktree 的身份、只读边界、权威数据和客户端入口没有关键歧义；实现不需要先引入 Project、Task、AI 或 Git 写操作。

## Phase 1：Workspace 与 Repository Registry

- `taskd` skeleton、SQLite schema、migration、CommandReceipt 和 DomainEvent/outbox；
- Workspace create/list/show/rename/archive；
- Repository 自动发现、手动注册、canonical path/remote identity/provider、重复检测、missing/moved 状态和受审计 unlink；
- Worktree Registry：canonical real path、Branch/HEAD、detached/dirty/missing 只读状态；
- rescan 只读取 Git 和文件系统事实，不执行 checkout、clean、commit、merge、push，也不删除目录；
- 至少一个可用的 TUI/GUI onboarding 与 Workspace/Repo 概览，另一客户端可后续补齐但必须复用同一契约；
- 本地 metadata backup/restore，包括全 writer gate、完整性 manifest、启动 reconcile、restore bootstrap journal 与原子 data-root generation 切换。

退出条件：用户能把包含多个 Repo/Worktree 的真实本地工作目录纳入 Workspace；重启、路径异常和备份恢复后 registry 身份稳定；重复注册被阻止；unlink 不触碰用户文件；所有 Git 操作均为只读。

## Phase 2：本地任务与 Review 闭环

- Task、Subtask、Actor、Ownership、TaskRelation 与 TaskContextBinding；Task.workspaceId 必填，可关联零个或多个 Repository/Worktree，不引入 Project 前置层；
- Inbox/Ready/In Progress/Blocked/Review/Done 状态机、Next、SavedView 和时间线；
- TUI/GUI 的快速捕获、键盘导航、列表/看板、依赖、详情、Review 和版本冲突界面；
- 评论、通用 Artifact/ArtifactLink、独立持有 `review_evidence` Link 的 ReviewSubmission/ReviewDecision；
- Blob pending + durable finalize intent + publish/finalize、deleting/deleted GC claim、崩溃恢复和孤儿清理；
- backup 扩展到 Artifact manifest/pin、全部 Blob hash、可移植 key envelope 与非终态 operation normalization；
- recurring/template、批量编辑和通知等增强只有在主闭环稳定后才进入 2.x 后续迭代。

退出条件：用户只靠该产品即可在真实 Workspace/Repo 上持续完成捕获、分流、执行、阻塞、版本化提交、验收/返工、重新打开和归档；source Link 变化不影响 submission-owned evidence；TUI/GUI 不产生状态分叉；Artifact 和 backup 故障注入可幂等收敛。

## Phase 3：AI 执行增强

- 通用 AI Actor、Assignment、Session、Invocation 和 AgentRun；
- `steward-mcp` 与一个首选 Runtime Adapter；
- 每个 Assignment 最多一个 starting/active AgentRun，sendPrompt/status/close 使用唯一目标；
- RuntimeOperation、稳定 operationId/requestHash、ordering scope、reconcile 和故障注入测试；
- ContextWindow、最小加密 History/WorkingNote、ContextCheckpoint 与 ContextBrief；
- capability、owner epoch、最小审批、审计，以及 Artifact/Blocked/Review handoff；
- 需要时加入绑定既有 Repository/Worktree 的 Git inspect；Git 写操作仍不在本阶段。

退出条件：人类和 AI 使用同一 Task；上下文窗口可重置且不丢失 Workspace/Task 权威状态；AI 完成不能直接 Done；外部 Runtime 副作用在响应丢失或 taskd 崩溃后不会重复执行。

## Phase 4A：BusinessFact 与 Workspace Knowledge

- 在既有 Workspace 上增加 BusinessSystem、Scenario、BusinessFact、FactRevision、Requirement、Decision 和 Glossary；
- FactRevision candidate/confirm/reject/supersede，以及并发安全的版本比较；
- Task/Scenario/BusinessFact 关联、业务 Wiki、场景图和影响范围查询；
- TUI/GUI 中的事实提案、确认、历史浏览和 ArtifactLink 正文。

退出条件：用户无需代码扫描或 Optimizer，即可在既有 Workspace 中维护确认事实并关联 Task/Repository；Branch/Worktree 切换不会改写长期事实身份。

## Phase 4B：实现快照、映射与漂移

- 在 Phase 1 Repository/Worktree Registry 之上增加 Component、CommitSnapshot、WorktreeSnapshot 和实现扫描；
- durable CaptureOperation 固定完整 Artifact intent 清单，通过 start/evidence/end state token 后原子发布 Snapshot/Link/Observation；
- ImplementationReference、不可变 Revision、ImplementationMapping 和带上下文的 MappingObservation；
- DriftFinding 的发现、分类、创建 Task/FactRevision candidate、解决和历史保留；
- AI Scanner 只生成 snapshot、candidate mapping/observation 和 finding。

退出条件：提交树与未提交现场可独立复核；Blob 或 token 失败不会发布半完成 Snapshot；MappingObservation 的 Revision 归属、observed snapshot 和 expected context 约束可验证；不同 Branch/Worktree 不串扰 freshness。

## Phase 5：Preference 与 Optimizer

- Preference、Feedback，以及从已确认事实和任务历史派生的候选；
- 重复澄清、返工、阻塞、工具失败和上下文浪费指标；
- Prompt、流程、模板和 Skill 候选；
- 用户确认、replay/shadow/canary 和回滚；
- ContextBrief 按任务选择已确认信息，避免全量注入。

退出条件：优化候选不自动生效，并能以证据证明至少一类效率损耗下降；禁用 Optimizer 后，Workspace、Task、AI 和 BusinessFact 能力仍完整可用。

## Phase 6：高级集成与强化安全

- 更多 Runtime Adapter 与 conformance suite；
- 完整 Session Store、多宿主 importer、长期全文索引/embedding；
- Git commit/merge/push 的 Plan → Approve → Execute → Verify；
- credential broker、Agent 隔离和 Hardened 模式；
- 插件、Policy Pack、签名与兼容策略。

退出条件：扩展能力不会绕过 taskd，敏感凭据和高风险操作满足对应威胁模型。

## 发布建议

- `0.1.x`：Workspace + Repository/Worktree Registry。
- `0.2.x`：本地 Task + Review + Artifact 闭环。
- `0.3.x`：一个 AI Runtime 的完整任务执行闭环。
- `0.4.0`：BusinessFact 与 Workspace Knowledge。
- `0.4.1`：实现快照、映射与漂移。
- `0.4.2`：Preference 与优化候选。
- `0.5.x`：更多 Runtime、Git 治理和 Hardened 能力。
- `1.0.0`：Workspace/Repository/Task schema、客户端契约、迁移和扩展 API 稳定。

每个版本必须保留上一层独立价值：没有 Task 时 `0.1` 仍是可靠 registry；AI 不可用时 `0.3+` 仍能管理任务；Optimizer 不可用时 `0.4+` 仍能维护事实与实现认知。
