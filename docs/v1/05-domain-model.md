# 领域与数据模型

## 1. 任务管理核心实体

### Project

- `projectId`、name、description
- status、defaultViewId
- createdAt、updatedAt、archivedAt
- `currentVersion`

Project 是任务的组织边界，不承担任务状态机职责。

### Task

- `taskId`、projectId、parentTaskId
- title、description、priority
- status、ownerActorId、nextAction
- acceptanceCriteria、dueAt
- blockedReason、reviewSummary
- createdAt、updatedAt、completedAt、archivedAt
- `currentVersion`

Task 是产品第一等 aggregate。子任务仍然是 Task，通过 `parentTaskId` 组织，不引入另一套生命周期。

### TaskRelation

- `relationId`
- fromTaskId、toTaskId
- type：`blocks` / `relates_to` / `duplicates`
- createdBy、createdAt

`blocks` 关系必须做自引用和循环检查。父子关系与阻塞关系语义分离。

### Actor

- `actorId`
- type：`human` / `ai` / `automation` / `service`
- displayName、status、metadata

Actor 统一表达 Owner 和 Worker 的身份，核心 Task 模型不写死具体 AI 宿主。

### TaskOwnership

- taskId、ownerActorId
- assignedBy、assignedAt
- ownerEpoch、endedAt

一个活跃 Task 同时最多有一个当前 Owner。换 owner 时递增 epoch，避免旧执行者继续写入。

### TaskEvent

- `eventId`、sequence
- taskId、eventType、actorId
- entityVersion、payload、idempotencyKey
- correlationId、causationId、createdAt

Task Event 是不可变时间线和同步依据。Task 当前状态仍由事务化表维护，不要求首版采用完整 Event Sourcing。

### Comment / Artifact

- Comment：commentId、taskId、authorActorId、body、createdAt、editedAt
- Artifact：artifactId、taskId、type、contentHash、storagePath、provenance、createdBy、createdAt

Artifact 可以是附件、链接、报告、测试结果或交付物；大内容存本地文件，SQLite 保存元数据。

### SavedView

- `viewId`、name
- filters、sort、groupBy
- displayMode、createdBy、currentVersion

TUI 与 GUI 共享相同筛选语义，但可以采用不同布局。

### ReviewDecision

- `reviewId`、taskId、reviewerActorId
- decision：`accepted` / `changes_requested`
- evidenceRefs、comment、createdAt

只有 accepted 决策可以把 Review 推进到 Done。

## 2. 任务状态模型

```text
Inbox ──triage──> Ready ──start──> In Progress ──submit──> Review ──accept──> Done
  │                  ▲                  │   ▲                 │
  └──cancel──────────┴──────────────────┴───┴──cancel─────────┘
                                         │
                                         └──block──> Blocked
                                                     │
                                                     └──resume──> Ready / In Progress

Any non-archived state ──archive when allowed──> Archived
```

不变量：

- Ready 前必须具备可理解的目标；建议具备 owner、下一步和验收标准。
- In Progress 必须有当前 Owner。
- Blocked 必须记录原因，可选记录依赖任务或等待对象。
- Worker 的完成声明只产生提交 Review 的事件。
- Done 必须关联 accepted ReviewDecision；重新打开会生成新事件和版本。
- Archived 是存储状态；原业务终态仍保留用于查询与统计。

## 3. 角色职责

### Task Manager

管理任务池：分流 Inbox、排序、拆分、建立依赖、分配 owner、发现停滞、升级阻塞和选择下一任务。它可以是用户承担的产品角色，后续也可以由受限 AI 辅助。

### Task Owner

对单个 Task 的推进负责：维护下一步、协调 Worker、报告阻塞、收集产物并提交 Review。Owner 不等于执行者，也不拥有绕过验收的权限。

### Worker / Reviewer

Worker 执行工作并提交证据；Reviewer 根据 acceptanceCriteria 做独立决定。一个 Actor 可以在不同任务中承担不同角色，但同一高风险任务可要求角色分离。

## 4. 第二阶段：AI 执行扩展

### Assignment

- assignmentId、taskId、stage、role
- assignedActorId、allowedOperations、allowedPaths
- ownerEpoch、status、currentVersion
- reportArtifactId、createdAt、completedAt

### Session / Invocation / AgentRun

- Session：可跨进程恢复的 AI 上下文身份；
- Invocation：Session 的一次实际运行；
- AgentRun：连接 Assignment、Session 与 Invocation，并记录 outcome。

### ContextBrief

- 当前目标、下一步和验收标准；
- 相关 Requirement、Decision 与 Preference 引用；
- Task、Artifact 和 Git 的权威事实引用；
- 上下文生成时间和版本。

这些实体扩展 Task 的执行证据，不替代 Task 本身。

## 5. 第三阶段：需求与优化扩展

### Requirement / Decision / Preference

- id、scope、content、sourceRefs
- status：`candidate` / `confirmed` / `rejected` / `superseded`
- confidence、confirmedBy、currentVersion

Preference 的 scope 至少区分 global、project、task-type 和 task-local。用户明确表达与 AI 推断必须有不同 provenance。

### Feedback / OptimizationProposal

- Feedback 关联 Review、返工、澄清或阻塞事件；
- OptimizationProposal 包含目标版本、证据、差异、风险、评估和回滚计划；
- PromptTemplate 可处于 candidate、canary、live、retired，但 live 变更需要用户确认。

## 6. 关键关系

```text
Project 1──N Task
Task 1──N Subtask
Task N──N Task                  via TaskRelation
Actor 1──N TaskOwnership
Task 1──N TaskEvent / Comment / Artifact / ReviewDecision
Task 1──N Assignment            second stage
Assignment 1──N AgentRun        second stage
TaskEvent N──N Requirement / Preference / Proposal evidence refs
```

## 7. 并发、事务与保留

- 所有可变 aggregate 包含 `currentVersion`，Command 必须带 `expectedVersion`。
- SQLite transaction 原子提交 aggregate 状态、关系变化和 Task Event/outbox。
- owner 变更使用 owner epoch；第二阶段的长操作再增加 lease 与 heartbeat。
- 事件和 ReviewDecision 默认不可原地改写；更正通过补充事件表达。
- 删除附件时保留最小元数据和引用状态，具体保留期限由数据策略决定。
