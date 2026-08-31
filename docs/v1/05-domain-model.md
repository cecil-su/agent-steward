# 领域与数据模型

## 1. 核心实体

### Task

- `taskId`
- 标题、项目、状态、阶段
- blocker、nextAction
- parentTaskId / batch relation
- ownerGrantId
- currentVersion

### Session

可跨进程恢复的 AI 上下文身份：

- `sessionId`
- provider/client/model
- parentSessionId
- createdAt/lastSeenAt/closedAt
- sessionSource

### Invocation

Session 的一次实际运行：

- `invocationId`
- sessionId
- runtime、process/pane/tab
- cwd、host
- startedAt/endedAt/exitReason

### Assignment

Task Owner 派发的阶段工作：

- `assignmentId`
- taskId、stage、role
- worktreeId、reportArtifactId
- allowedOperations、allowedPaths
- ownerEpoch、status、currentVersion

### AgentRun

连接 Assignment、Session 与 Invocation：

- assignmentId
- sessionId
- invocationId
- startedAt/completedAt
- outcome、completionEventId

## 2. 权限实体

### Principal

人类、AI Session、服务或 Runtime Adapter 的安全身份。

### RoleGrant

由 issuer 向 subject 授予有 scope 的角色。

### Capability

面向单个 connection 或短期操作的可撤销凭据。

### Lease

记录 owner/writer 等独占权的期限、heartbeat 和 epoch。

## 3. 运行与证据实体

### Event

- eventId、type、assignmentId
- deliveryState：pending/delivered/processed
- payloadHash
- createdAt/ackedAt
- idempotencyKey

### Artifact

- artifactId、type、contentHash
- storagePath/encryption metadata
- createdBy、taskId、assignmentId
- provenance、retentionClass

### ContextCheckpoint

- Session/Assignment 当前总结
- 权威事实引用
- Git fingerprint
- 恢复触发条件
- prompt/template version

### ReviewEvidence / TestEvidence

独立记录执行者、目标 SHA/diff、命令、退出码、结果和 finding。

## 4. Git 实体

### Repository

- repositoryId
- canonicalPath/remote identity
- provider
- policyId

### Worktree

- worktreeId
- canonicalRealPath
- repositoryId、branch
- taskId、writerLeaseId
- managed/unmanaged

### GitPlan

- planId、operationType
- taskId、repositoryId、worktreeId
- inputSnapshot
- expectedResult
- planHash、expiresAt、status

### GitOperation

- operationId、planId
- executor、startedAt/completedAt
- stdout/stderr artifact
- resultSnapshot
- reconciliationState

## 5. Prompt 与优化实体

### PromptTemplate

- templateId、version
- role/stage/runtime
- template content artifact
- status：candidate/canary/live/retired

### PromptInstance

- template version
- structured inputs
- rendered content hash/artifact
- assignmentId/sessionId

### OptimizationObservation

确定性统计或模型分析产生的观察，绑定证据来源。

### OptimizationProposal

- target artifact/version
- evidence
- proposed diff
- risk/confidence
- evaluation/rollback plan
- authorization level

### Experiment

记录 replay、shadow、canary 和结果对比。

## 6. 关键关系

```text
Task 1──N Assignment
Assignment 1──N AgentRun
Session 1──N Invocation
AgentRun N──1 Session/Invocation/Assignment
Task 1──N GitPlan/GitOperation
Assignment 1──N Artifact/Event/Evidence
PromptTemplate 1──N PromptInstance
OptimizationProposal 1──N Experiment
```

## 7. 并发与版本

所有可变 aggregate 包含 `currentVersion`。写入请求必须带 `expectedVersion`，避免多个 Session 静默覆盖。

关键独占资源使用 lease：

- Registry Main lease
- Task Owner lease/epoch
- Worktree Writer lease
- Git operation lease

SQLite transaction 保证状态和 outbox event 原子提交。
