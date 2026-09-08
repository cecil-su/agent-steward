# 领域与数据模型

> 范围：阶段 A/B 只采用任务接续需要的 Workspace/Repo 身份、Task、Actor/Ownership、TaskCheckpoint、Review、Receipt/Event 和实际启用的证据存储。TaskRelation、SavedView 按需求启用；Assignment/AgentRun/ContextWindow 属于可选阶段 D，BusinessFact/Mapping 属于探索 X。模型存在不等于首版必须实现。
>
> 本文 taskd 表示可信应用核心；是否部署为常驻服务由 D-019 决定。Blob 发布协议从首次使用 Blob 起生效；在线 BackupOperation/Pin 协议按在线能力启用，首期维护备份仍必须验证整套数据。Review 编辑与撤回采用第 21 篇已冻结设计，待实现验证；restore generation、Runtime 重试目标和事实版本绑定仍待 D-021–D-023 关闭。

## 1. Workspace / Repository 基础与任务核心实体

### Workspace

- `workspaceId`、name、description、rootPath
- status：`active` / `archived`
- defaultViewId
- createdAt、updatedAt、archivedAt
- `currentVersion`

Workspace 是本地数据与工作上下文的第一等边界，随阶段 A 任务创建存在。Repository、Worktree、Task 和后续 BusinessFact 都必须归属 Workspace；V1 不要求用户创建 Project，也不以 Project 作为 Task 的必经父级。rootPath 用于 discovery scope，不授予删除或移动目录的权限。系统可以自动建立默认 Workspace，用户不需要先完成 Registry onboarding；默认选择和冲突规则见 D-018。

### Repository / Worktree Registry

- Repository：repositoryId、workspaceId、canonicalRealPath、remoteIdentity、provider、availabilityState、lastScannedAt、currentVersion
- Worktree：worktreeId、repositoryId、canonicalRealPath、branch、headSha、availabilityState、dirtyState、managed、lastScannedAt、currentVersion
- availabilityState：`available` / `missing` / `moved` / `identity_conflict` / `unlinked`
- dirtyState：`clean` / `dirty` / `unknown`

Repository identity 由规范化 real path 与可用时的 Git identity 共同校验，不能仅按用户输入字符串创建重复记录。Worktree 必须属于同一 Repository；detached HEAD 以 branch 为空和 headSha 表达，不伪造 Branch。阶段 A 的按需 discover/register/rescan 只读取文件系统和 Git 事实；unlink 是受审计的 registry 状态转换，不删除、移动或清理 Repository/Worktree 目录。missing/unlinked 记录保留最小 identity 与历史引用，重新发现时必须显式 reconcile，不能按路径猜测为新对象。

### Task

- `taskId`、workspaceId、parentTaskId
- title、description、priority
- status、archiveState、nextAction
- acceptanceCriteria、dueAt
- blocker（kind、reason、requiredActorId/requiredRole、requiredInput、resumeCondition、evidenceRefs、createdAt/createdBy）；无当前阻塞时为空
- reviewSummary 为派生查询，不作为可写 Task 字段
- createdAt、updatedAt、completedAt、archivedAt
- `currentVersion`

Task 是阶段 A 的交付核心，属于 Workspace 内的工作 aggregate。workspaceId 必填；parentTaskId/dueAt 是按需启用字段，不构成首版必填。启用子任务后仍使用 Task，通过 parentTaskId 组织，且必须与父 Task 属于同一 Workspace，不引入另一套生命周期。

### TaskCheckpoint（阶段 A）

- checkpointId、taskId、taskVersion、ownerActorIdAtCapture、ownerEpochAtCapture
- progressSummary、nextAction、openQuestions、decisionRefs、evidenceRefs
- gitObservationRefs（含 repository/worktree、HEAD、dirtyState、observedAt）
- status：`complete` / `partial`；missingRefs
- createdByActorId、createdAt、schemaVersion
- sourceSessionRef 可选，仅作来源，不作为权限或归属依据

TaskCheckpoint 是不可变恢复记录；create 使用 Task expectedVersion 和幂等键，在事务中验证引用并保存记录与事件。Checkpoint 不改变任务生命周期，也不自动关闭 Task。owner 字段仅表示采集时观察，当前 ownership 仍由 active TaskOwnership 派生。

恢复时重新读取当前 Task、权限和 Git，展示它们与 Checkpoint 的差异；progressSummary、nextAction 是采集时执行者声明，不能覆盖更新后的 Task。partial 必须列出缺失证据。TaskCheckpoint 不要求 Assignment、Session、ContextWindow、AI 或 Runtime 存在；与阶段 D 的 ContextCheckpoint 分开，后者可以引用前者。

Checkpoint 追加不递增 Task.currentVersion，记录自己的创建事件但仍校验 Task expectedVersion；详细规则见第 21 篇。物理证据存储在 D-024 冻结，文件路径本身不能替代固定的 Review evidence。

Task.blocker 的字段、block/unblock 与 needs-input 派生视图见[补充合同](21-continuity-and-review-contracts.md)。当前阻塞只有 Task 这一权威，旧原因和解除记录保留在 TaskEvent。

### TaskRelation（按需求启用）

- `relationId`
- fromTaskId、toTaskId
- type：`blocks` / `relates_to` / `duplicates`
- createdBy、createdAt

`blocks` 关系必须做自引用和循环检查。父子关系与阻塞关系语义分离。

### TaskContextBinding

- bindingId、taskId、workspaceId
- targetType：`repository` / `worktree`
- targetId、relationType：`primary` / `related`
- currentVersion、createdAt、unlinkedAt

Task 可以关联零个或多个同一 Workspace 内的 Repository/Worktree；没有具体代码载体的 Task 仍可只属于 Workspace。binding 创建和变更必须验证 target 的 workspace 归属及 expectedVersion，不能借 binding 跨 Workspace 扩大 Artifact、AI 或 Git scope。同一 `(taskId, targetType, targetId)` 最多一个 active binding；解除 binding 不修改 Git 或文件系统。

### Actor

- `actorId`
- type：`human` / `ai` / `automation` / `service`
- displayName、status、metadata

Actor 统一表达 Owner 和 Worker 的身份，核心 Task 模型不写死具体 AI 宿主。

Actor 是领域身份，不是客户端可声明的认证凭据。Command 的 acting Actor 由 taskd 根据认证 Principal 和受保护 binding 派生；Assignment/Ownership 中的 `targetActorId` 是另行授权的操作目标。

### TaskOwnership

- ownershipId、taskId、ownerActorId
- assignedBy、assignedAt、endedBy、endedAt
- ownerEpoch

TaskOwnership 是 Task aggregate 内的 ownership 历史，也是 Owner 的唯一写入权威；Task 不再保存 ownerActorId。当前 Owner 由 `endedAt IS NULL` 的 active row 派生，数据库使用 `UNIQUE(taskId) WHERE endedAt IS NULL` 保证每个 Task 最多一个 active ownership。换 owner 时在同一 Task transaction 中结束旧 row、插入 ownerEpoch + 1 的新 row、递增 Task.currentVersion 并写 TaskEvent；所有 owner Query 和权限检查都读取 active TaskOwnership，不维护第二份缓存列。

### DomainEvent / TaskEvent

- `eventId`、streamPosition、schemaVersion
- aggregateType、aggregateId、aggregateVersion
- principalId、actorId、eventType、payload
- correlationId、causationId、idempotencyKey、requestHash、createdAt

DomainEvent 是所有 aggregate 共用的不可变同步、恢复和审计 envelope。`streamPosition` 是由 SQLite 写事务分配的全局唯一、单调递增位置，与 aggregateVersion 分离；允许因内部记录或回滚预留而出现空洞，客户端不能把它解释为 aggregate 版本。TaskEvent 是 `aggregateType=task` 的事件族，不再强制通用事件包含 taskId。FactRevision、ImplementationMapping、MappingObservation、Snapshot 和 DriftFinding 的变化使用同一 envelope。

schemaVersion 是必填正整数，按 eventType 标识 payload schema；生产者只能追加新版本，消费者必须显式支持、upcast 或拒绝未知版本，不能按当前结构猜读旧 payload。当前 aggregate 状态仍由事务化表维护，不要求首版采用完整 Event Sourcing。每个事件的 principalId 和 actorId 由 taskd 根据认证 connection 派生。

### CommandReceipt / IdempotencyRecord

- principalId、commandType、idempotencyKey、requestHash
- commandSchemaVersion、targetRefs、status、resultRef、correlationId
- createdAt、completedAt、retainUntil

唯一键为 `(principalId, commandType, idempotencyKey)`。taskd 对规范化后的语义请求计算 requestHash；规范化内容包含 command schema version、target、payload、expectedVersions 和服务端绑定 scope，不包含 correlationId 或传输层噪声。同一 key + 同一 requestHash 只保证不重复执行副作用，不能绕过当前授权：返回历史 status/resultRef 前必须重新校验当前 connection、grant 以及结果对象的读取权限。授权已撤销或缩小时返回 `FORBIDDEN_SCOPE`，或仅返回不含 payload/resultRef 的脱敏终态；同一 key + 不同 requestHash 返回 `IDEMPOTENCY_KEY_REUSED`，不得执行或覆盖旧记录。DomainEvent 和外部 operation 记录保存同一 requestHash 以支持审计与恢复。Receipt 在关联 Intent/Operation 未终结时不得过期；完整结果超过 retainUntil 后仍保留 key + requestHash + terminal status/resultRef tombstone，V1 不把旧 key 静默绑定到新请求。

### EvidenceDescriptor / ReviewSummary（阶段 A/B）

EvidenceDescriptor 是 Artifact 的可选不可变语义元数据，字段和来源校验见第 21 篇；没有独立 Evidence ID 或状态机。descriptorHash 与 contentHash 一起固定到 ReviewSubmissionEvidence/evidenceSetHash。重新采集或更正新建 Artifact，不能修改原 descriptor。

ReviewSummary 从 Submission、Evidence 和 Decision 投影，不新增 Handoff aggregate。提交时证据与当前现场分开展示，交付事实和用户决定不能由生成摘要反向写入。

### Comment / Artifact / ArtifactLink

- Comment：commentId、taskId、authorActorId、kind、body、sourceRefs、confirmationStatus、createdAt；kind 候选为 progress/decision/constraint/procedure，首版追加记录，更正通过新记录引用原条目。confirmationStatus 由受控用户确认派生，AI 不能自行写 confirmed；具体字段和复用规则在 D-024 冻结。
- Artifact：artifactId、type、mimeType、evidenceDescriptor（可选）、descriptorHash（可空）、contentHash、blobHash、size、storageState、stagingName、storagePath、ingestOperationId、deletionOperationId、failureReason、provenance、createdBy、currentVersion、createdAt、finalizedAt、failedAt、deletingAt、deletedAt
- storageState：`pending` / `finalized` / `orphaned` / `failed` / `deleting` / `deleted`
- ArtifactLink：artifactLinkId、artifactId、aggregateType、aggregateId、relationType、linkState、supersededByLinkId、currentVersion、createdBy、linkedAt、supersededAt、unlinkedBy、unlinkedAt
- linkState：`active` / `superseded` / `unlinked`
- ArtifactFinalizeIntent：intentId、artifactId、ingestOperationId、targetAggregateType、targetAggregateId、commandType、commandSchemaVersion、canonicalCommand、requestHash、expectedVersions、relationType、newLinkId、priorLinkId、expectedPriorLinkVersion、principalId、actorId、idempotencyKey、correlationId、causationId、intentState、failureReason、currentVersion、createdAt、expiresAt、appliedAt
- intentState：`prepared` / `blob_published` / `applied` / `conflicted` / `cancelled` / `failed`

Artifact 是通用内容对象，可以是附件、链接、报告、测试结果、FactRevision/Wiki 正文或 Snapshot evidence；不包含 taskId 等业务归属字段。Task、Assignment、ReviewSubmission、FactRevision、CommitSnapshot/WorktreeSnapshot、WorkingNote 和其他 aggregate 统一通过 ArtifactLink 关联 Artifact；Wiki 页面按作用域链接 Workspace、BusinessSystem、BusinessScenario 或 BusinessFact。relationType 表达 `attachment`、`report`、`review_evidence`、`content`、`wiki_page`、`index_manifest`、`staged_diff`、`unstaged_diff`、`untracked_manifest` 等用途。

大内容存本地文件，SQLite 保存 Artifact 元数据、ArtifactLink、ArtifactFinalizeIntent 和 CaptureArtifactIntent。同一 Artifact 可链接多个 aggregate；只有 finalized Artifact 可以创建 active Link。普通读取只接受 active Link 并按目标 aggregate scope 校验，不能仅凭 artifactId 或已 superseded/unlinked Link 绕过授权；历史读取另需 retention/audit capability。

V1 中一个 Artifact 独占一个物理 Blob/storagePath，数据库对 storagePath 建唯一约束；不允许多个 Artifact 行共享同一物理 Blob。contentHash 不唯一。内容复用只能在授权后给同一个 finalized Artifact 增加 ArtifactLink；若创建新的 Artifact，则写入独立 Blob。该约束使 GC 的所有权和删除范围唯一。

Artifact 允许 `pending → finalized/orphaned/failed`，以及 `finalized/orphaned/failed → deleting → deleted`；deleting/deleted Artifact 不能新增 Link。ArtifactLink 只允许 `active → superseded` 或 `active → unlinked`，终态 Link 不重新激活；重新关联必须创建新 Link。所有 Link transition 使用 expectedVersion，并写 DomainEvent。要求单一当前正文的 relationType 在对应 aggregate 内最多一个 active Link。

#### Artifact Blob 发布协议

文件系统不参与 SQLite transaction。单 Artifact 的附件、WorkingNote/FactRevision/Wiki 正文和 Snapshot evidence 使用以下 publish-before-reference 协议；一次 WorktreeSnapshot 所需的多 Artifact 集合使用后文 CaptureOperation 变体，不能逐项执行第 4 步而暴露半完成 Snapshot：

1. taskd 在与最终 Blob 相同文件系统的 staging 目录流式写入临时文件，完成加密并计算 contentHash、加密后 blobHash 与 size；文件和 staging 目录完成 flush/fsync 后才进入下一步。部分写入不得复用。
2. 第一个 SQLite transaction 创建 `storageState=pending` 的 Artifact、CommandReceipt，以及 `intentState=prepared` 的 durable ArtifactFinalizeIntent。Intent 保存服务端规范化 canonicalCommand、requestHash、目标 aggregate、全部 expectedVersions、预分配 newLinkId、priorLinkId/expectedPriorLinkVersion、身份/关联字段和到期时间，足以在没有原进程内存的情况下精确完成或终止请求；同时写 internal/audit-only 的 ArtifactBlobPending 存储审计记录。该记录没有业务 Event Stream 的 streamPosition，不向普通客户端投递，其可见范围由 Intent target scope 控制；此事务不得创建 ArtifactLink、更新目标业务 aggregate 或写目标业务变化事件。
3. taskd 校验临时文件的 blobHash/size，再原子 rename 到最终路径并 fsync 父目录。目标已存在时必须校验内容一致后按幂等成功处理；staging 与最终路径必须位于支持原子 rename 的同一文件系统。随后以短 SQLite transaction 把 Intent 标记为 blob_published；若在标记前崩溃，恢复器可由最终文件及 hash 确认已发布。
4. finalize transaction 只读取 durable Intent，并重新验证 requestHash、CommandReceipt、授权/撤销状态、全部 expectedVersions、priorLink 状态和 Artifact 版本。全部匹配时才把 Artifact 改为 finalized、Intent 改为 applied，同时创建或切换 ArtifactLink、更新目标 aggregate/currentContentLinkId、完成 CommandReceipt、递增版本，并写 ArtifactFinalized、Link/目标 aggregate 的 DomainEvent/outbox。
5. 若业务版本、旧 Link、权限或 normalized request 已变化，Intent 进入 conflicted；Intent 到期或调用者取消则进入 cancelled。CommandReceipt 在同一 SQLite transaction 中记录确定性终态，未引用的 Artifact 进入 orphaned，无论 Blob 尚在 staging 还是已经发布；这些结果不自动重试。Blob 缺失或 hash/size 校验失败则 Intent/Artifact 进入 failed。只有未到期的 prepared/blob_published Intent 可以自动恢复，且每次恢复仍执行第 4 步的全部 compare-and-set 前置检查。
6. 启动恢复扫描未完成 Intent：最终 Blob 有效则补记 blob_published 并尝试第 4 步；仅 staging 文件有效则重放第 3、4 步；两者均缺失或校验失败则按第 5 步失败。没有 metadata 的过期 staging 文件由受审计 GC 清理。

因此崩溃不会产生指向未发布 Blob 的业务对象；允许出现的中间结果只有不可见 pending Artifact 或可回收 orphaned Blob。恢复依据 durable Intent、CommandReceipt 和 requestHash，而不是重新解释客户端请求；conflicted/cancelled/failed Intent 不进入永久重试。

复用已经 finalized 且校验通过的 Artifact 时跳过 staging/publish，只在目标 aggregate 的 SQLite transaction 中创建新 active Link，并在同一 transaction 校验 Artifact 仍为 finalized。

#### Artifact Blob GC 协议

1. GC 在 SQLite transaction 中确认没有 active Link 或 active BackupArtifactPin，且所有 superseded/unlinked Link 已超过 retention，再以 Artifact.currentVersion 做 compare-and-set，把 finalized/orphaned/failed Artifact 改为 deleting，分配 deletionOperationId 并写 ArtifactDeletionClaimed 事件。创建 pin 与 GC claim 由同一 SQLite 写序列化，先提交者使后者的前置检查失败。
2. 所有新 Link transaction 都必须读取并锁定/验证 Artifact.storageState 与 currentVersion；只允许 finalized。SQLite 串行化保证 Link 先提交时 GC claim 失败，GC 先提交时 Link 创建失败，因此 claim 后不会出现新 Link。
3. GC 在事务外按 deletionOperationId 幂等删除独占 storagePath 和残留 stagingName，并 flush/fsync 对应父目录；随后在 SQLite transaction 中把 deleting 改为 deleted、保留最小 hash/审计元数据并写 ArtifactDeleted 事件。
4. 重启发现 deleting 时：文件存在则继续删除，文件不存在则直接完成 deleted。deleted Artifact 永不重新 Link；需要相同内容时必须重新 ingest 为新 Artifact。

### BackupOperation / BackupArtifactPin / RestoreBootstrapJournal（在线扩展）

首期采用 D-019 的显式维护停写备份；以下 writer gate / pin 协议用于在线备份扩展。恢复的隔离验证、外部副作用不自动重放和整套数据切换仍适用于维护备份。跨恢复 generation 的请求失效契约尚待 D-021 关闭，本节不能据此视为完整冻结。

- BackupOperation：backupId、destinationRef、temporaryRef、backupSchemaVersion、sourceDataRootGeneration、state、eventWatermark、artifactManifestHash、databaseHash、keyEnvelopeHash、finalManifestHash、failureReason、currentVersion、createdAt、completedAt
- state：`preparing` / `copying` / `verifying` / `publishing` / `complete` / `failed` / `cancelled`
- BackupArtifactPin：backupId、artifactId、expectedBlobHash、expectedSize、pinState、createdAt、releasedAt
- pinState：`active` / `released`
- RestoreBootstrapJournal：operationId、principalId、idempotencyKey、requestHash、backupId、backupManifestHash、oldDataRootGeneration、newDataRootGeneration、oldDataRootRef、newDataRootRef、phase、resultRef、failureReason、createdAt、updatedAt、completedAt
- restore phase：`prepared` / `staged` / `switched` / `validated` / `complete` / `failed` / `rollback_required`

备份包通过以下协议取得 SQLite 与 Blob 的一致性：

1. taskd 在 SQLite connection/transaction 入口取得独占的短期 writer gate。在持有 gate 的初始化 write transaction 中创建 BackupOperation，记录当前全局 eventWatermark，为该一致性点所有保留中的 finalized Artifact 物化 manifest row 并创建 active BackupArtifactPin。GC claim 必须同时验证不存在 active pin。
2. writer gate 保持到 SQLite online backup 从该一致性点完成，除持有 gate 的初始化事务外禁止所有 SQLite writer。范围包括 Application Command、Artifact finalize/recovery、Runtime callback/reconcile、lease/heartbeat、event ack/retention、GC、其他 backup/recovery worker、migration 和维护事务；任何写路径都不得绕过数据库层 gate。数据库副本因此包含与固定时刻完全相同的 manifest/pin、Link 状态和 eventWatermark，期间恢复器不能把 pending Artifact finalize 成新的 active Link。
3. online backup 完成并校验数据库副本后释放 writer gate；无需在整个 Blob 复制期间阻塞普通写入。随后按已固定 manifest 从独占 storagePath 复制 Blob 到带 backupId 所有权标记的临时备份目录，并逐项校验 artifactId、blobHash 和 size。active pin 保证复制完成前 Artifact 不能进入 deleting。
4. 备份必须包含可移植的加密 key envelope：数据密钥由用户提供的备份口令/恢复密钥包装，或由显式配置的外部 key provider 包装；不得复制明文密钥，也不能把仅在原设备 OS Keychain 中有效的引用宣称为可移植备份。
5. 数据库文件、Blob manifest、eventWatermark、schema/version、key envelope 和各文件 hash 全部验证后，在临时目录写入并 fsync 最终完整性 manifest。随后先把 live BackupOperation 标为 `publishing` 并固定 finalManifestHash，再原子发布备份目录，最后把 operation 标为 `complete` 并释放 pin。失败/取消的临时备份不可列为可恢复备份；只有在按 backupId、temporaryRef 和 manifest hash 确认目录归属后才能清理。
6. taskd 启动时必须先 reconcile 所有非终态 BackupOperation：若 destinationRef 中已存在与 backupId/finalManifestHash 完全匹配且通过完整性校验的最终包，则补记 `complete` 并释放 pin；若只有归属明确且可恢复的临时目录，则从已持久化阶段继续；若两者均不可用或损坏，则记录 `failed`、释放 pin，再清理确认归属的临时目录。不得因“operation 尚未 complete”删除一个已经原子发布且有效的最终包，也不得让发布后的崩溃留下永久 active pin。
7. restore 的幂等与切换状态不能只写在即将被替换的 SQLite 中。taskd 在 old/new data root 之外的受保护 bootstrap 区追加或原子更新并 fsync RestoreBootstrapJournal，以 `(principalId, restore, idempotencyKey)` 唯一标识请求；同 key + 同 requestHash 必须按 journal 当前阶段继续或返回原 result，同 key + 不同 hash 拒绝执行。journal 至少在开始 staging、切换 data-root generation 前、切换后和启动校验完成后各持久化一次；complete 后仍按 CommandReceipt 保留规则保存 key、requestHash、resultRef 和 new generation tombstone，清理 old data root 不得删除它。
8. restore 在与现有数据隔离的临时根目录中解包并验证完整性 manifest、数据库 hash/`quick_check`、schema 兼容性、key envelope 可解包，以及每个保留中 ArtifactLink → Artifact → Blob 的存在性和 hash。验证成功后，在 staged database 的恢复事务中写 restore normalization audit，并执行以下 normalization，所有条目都不得在新机器自动重放外部副作用：`prepared/blob_published` ArtifactFinalizeIntent 以及非终态 CaptureOperation/CaptureArtifactIntent 改为 cancelled/failed，其未引用 Artifact 按 Blob 是否存在进入 orphaned/failed，关联的 pending CommandReceipt 以 `interrupted_by_restore` 终结；非终态 RuntimeOperation 进入 needs_reconciliation，`starting/active` AgentRun 进入 detached；lease、heartbeat 和短期 capability 全部过期或撤销；非终态 Git execution 进入 needs_reconciliation 且旧批准失效；包内源 BackupOperation 标为 complete 并释放其 manifest pin，其他非终态 BackupOperation 标为 failed/cancelled 并释放全部 pin。验证失败不得修改 live store。
9. restore 需要 taskd 停止或进入独占维护模式。staged root 验证和 normalization 完成后，把 journal 标为 `staged`，再通过原子切换 data-root generation 指针启用整套 SQLite + Blob Store，不在原位置逐文件覆盖；切换后把 journal 标为 `switched`，新实例启动校验通过后依次标为 `validated` 和 `complete`。启动必须先于普通 worker 读取 journal 与实际 generation：指针已指向 new generation 时只能继续校验/收尾，仍指向 old generation 时只能继续 staging 或安全失败，不能重新执行一次 restore。旧数据根保留到新实例校验完成后再按用户确认的策略处理；校验失败进入 rollback_required，由同一 journal 协调显式回切。

### SavedView（按需求启用）

- `viewId`、name
- filters、sort、groupBy
- displayMode、createdBy、currentVersion

TUI 与 GUI 共享相同筛选语义，但可以采用不同布局。

### ReviewSubmission / ReviewDecision

- ReviewSubmission：submissionId、taskId、reviewCycle、submittedTaskVersion、acceptanceCriteriaHash、evidenceSetHash、status、submittedByActorId、currentVersion、submittedAt、decidedAt、withdrawnBy、withdrawnAt、withdrawReason
- status：`pending` / `accepted` / `changes_requested` / `withdrawn`
- ReviewSubmissionEvidence：submissionId、artifactLinkId、artifactLinkVersion、sourceArtifactLinkId、sourceArtifactLinkVersion、artifactId、contentHash、descriptorHash、ordinal
- ReviewDecision：reviewDecisionId、submissionId、taskId、reviewCycle、reviewedTaskVersion、reviewedSubmissionVersion、acceptanceCriteriaHash、evidenceSetHash、reviewerActorId、decision、comment、createdAt
- decision：`accepted` / `changes_requested`

每次进入 Review 都创建新的 ReviewSubmission；`(taskId, reviewCycle)` 唯一且 reviewCycle 单调递增。submit transaction 校验 Task expectedVersion，并对每个客户端提交的 source Link 校验其仍为 active、expected source Link version、当前读取权限，以及 Artifact 仍为 finalized 且 contentHash/descriptorHash 匹配；随后为同一 Artifact 创建新的 active ArtifactLink：`aggregateType=review_submission`、`aggregateId=submissionId`、`relationType=review_evidence`。ReviewSubmissionEvidence.artifactLinkId/artifactLinkVersion 只引用这个由 Submission 持有的新 Link，sourceArtifactLinkId/sourceArtifactLinkVersion 仅保留提交时 provenance，原业务 Link 后续 supersede/unlink 不影响 Review evidence 的权限或保留。事务按 ordinal 和新 Link/Artifact/contentHash/descriptorHash 计算 canonical evidenceSetHash，创建 Submission/evidence rows 和全部 review_evidence Link，将 Task 推进到 Review，并把提交后 Task.currentVersion 记录为 submittedTaskVersion；任一证据校验或 Link 创建失败则整体回滚。

accept/request-changes Command 必须同时携带 expectedTaskVersion 和 expectedSubmissionVersion，并验证 `Task.currentVersion = ReviewSubmission.submittedTaskVersion`、验收条件 hash 与 evidence set hash 未变。accept 还必须逐项验证 ReviewSubmissionEvidence 指向的 Link 仍为 active，`aggregateType/aggregateId/relationType` 正确，Link.currentVersion 与 artifactLinkVersion 相同，且关联 Artifact 仍为 finalized、artifactId/contentHash/descriptorHash 与 evidence row 匹配；任一项不符都不得 accept。request-changes 可把证据缺失或不可读本身作为返工理由，不以 active Link 校验作为前置条件。新 Decision 的 reviewedTaskVersion/reviewedSubmissionVersion 固定为通过校验的两个版本。accept 在一个 SQLite transaction 中创建 accepted ReviewDecision、把 Submission 标为 accepted、把 Task 从 Review 推进到 Done、递增两个 aggregate 版本并写对应 DomainEvent；request-changes 以同样方式创建 Decision、把 Submission 标为 changes_requested 并把 Task 退回 In Progress。Done 重新打开后，旧 Submission/Decision 只保留为历史；再次进入 Review 必须创建更大的 reviewCycle，旧 accepted Decision 不能满足新的 Done 前置条件。

`review_evidence` Link 的普通读取权限由 ReviewSubmission → Task scope 派生，不再依赖 source aggregate。该 Link 不允许 supersede，也不得由普通 unlink Command 删除。只有受审计的 retention/delete workflow 在 Submission 及验收历史达到策略保留期后才能 unlink，之后才允许 Artifact GC；因此普通业务对象删除不会使待验收或保留期内的历史验收证据失去可读性。

Review 期间 Task 业务编辑必须先 withdraw；Comment/TaskCheckpoint 为不递增 Task version 的追加记录。withdraw 校验当前 Task/Submission 版本，原子终结 pending Submission 并退回 In Progress，不以 submittedTaskVersion 相等为前提。withdrawnBy/withdrawnAt/withdrawReason 与事件保留操作事实。完整权限、唯一 pending 约束和竞争规则见第 21 篇。

## 2. 任务状态模型

```text
Inbox ──triage──> Ready ──start──> In Progress ──submit──> Review ──accept──> Done
  │                  ▲                  │   ▲                 │
  └──cancel──────────┴──────────────────┴───┴──cancel─────────┘
                                         │
                                         └──block──> Blocked
                                                     │
                                                     └──resume──> In Progress

Review ──request changes / withdraw──> In Progress
Done ──reopen──> In Progress

archiveState: active ──archive──> archived ──restore──> active
```

不变量：

- Ready 前必须具备可理解的目标；建议具备 owner、下一步和验收标准。
- In Progress 必须有当前 Owner。
- Blocked 必须有结构化 blocker，block 从 In Progress 进入，unblock 记录 resolution 并回到 In Progress；其余前置条件见第 21 篇。
- Worker 的完成声明必须创建绑定 Task/criteria/evidence 版本的 ReviewSubmission，不能直接写 Done。
- Done 必须由当前 pending ReviewSubmission 的原子 accept transaction 产生；重新打开会使该 cycle 仅保留为历史，下一次验收必须使用新的 reviewCycle。
- Archived 不是生命周期 status。归档只把 `archiveState` 改为 `archived` 并设置 archivedAt，原 status（包括 Done、Cancelled 或其他允许归档的状态）保持不变。
- archived Task 默认不接受生命周期 Command；恢复为 active 后继续按保留的 status 处理。

## 3. 角色职责

### Task Manager

管理任务池：分流 Inbox、排序、拆分、建立依赖、分配 owner、发现停滞、升级阻塞和选择下一任务。它可以是用户承担的产品角色，后续也可以由受限 AI 辅助。

### Task Owner

对单个 Task 的推进负责：维护下一步、协调 Worker、报告阻塞、收集产物并提交 Review。Owner 不等于执行者，也不拥有绕过验收的权限。

### Worker / Reviewer

Worker 执行工作并提交证据；Reviewer 根据 acceptanceCriteria 做独立决定。一个 Actor 可以在不同任务中承担不同角色，但同一高风险任务可要求角色分离。

## 4. 可选阶段 D：AI 执行控制扩展

### Assignment

- assignmentId、taskId、stage、role
- assignedActorId、allowedOperations、allowedPaths
- ownerEpoch、status、currentVersion
- createdAt、completedAt

Assignment 报告通过 `relationType=report` 的 ArtifactLink 表达，不在 Assignment 中保存第二套 Artifact 外键。

### Session / Invocation / AgentRun

- Session：可跨进程恢复的 AI 上下文身份；
- Invocation：Session 的一次实际运行；
- AgentRun：agentRunId、assignmentId、sessionId、invocationId、runtimeId、runtimeHandleRef、status、outcome、currentVersion、startedAt、endedAt。

AgentRun status 为 `starting` / `active` / `completed` / `failed` / `cancelled` / `detached`。Assignment 可以保留多个历史 AgentRun，但 V1 每个 Assignment 最多一个 `starting/active` AgentRun；数据库使用等价的 partial unique constraint 保证该不变量。spawn/resume 在调用 Runtime 前，与 prepared RuntimeOperation 同一事务创建或保留 starting 行；成功后写入 runtimeHandleRef 并改为 active，失败/reconcile 后进入对应终态。首次 sendPrompt/status/close 由 taskd 根据 assignmentId 解析唯一 active AgentRun 和 runtimeHandleRef；副作用请求重试的持久目标匹配顺序仍须在 D-022 冻结，不能重新解析后作用于替代 Run。首次解析时，零个或多个候选都返回确定性错误，不能按“最新时间”猜测目标。

### RuntimeOperation

- operationId、operationType、assignmentId、sessionId、invocationId、runtimeId、orderingScope、effectSequence
- idempotencyKey、requestHash、canonicalRequest、externalOperationId、runtimeHandleRef
- state：`prepared` / `dispatching` / `succeeded` / `failed` / `unknown` / `needs_reconciliation`
- attempt、resultRef、failureReason、currentVersion、createdAt、dispatchedAt、completedAt

spawn、resume、sendPrompt、context transition 和 close 等外部副作用先在 SQLite 中创建 prepared RuntimeOperation，再把稳定 operationId 传给 Adapter/宿主。成功响应与 handle/result 在事务中持久化；进程在“宿主成功、SQLite 未记录”窗口崩溃时，taskd 必须按 operationId 调用 reconcile，不得直接重复副作用。宿主无法按 operationId 查询或去重时，operation 进入 needs_reconciliation，由用户/Adapter 发现既有 Session/Prompt 后确认，不自动重试。

同一 Assignment 的 spawn/resume orderingScope、同一 RuntimeHandle 的 prompt/control orderingScope 在前一 operation 为 unknown/needs_reconciliation 时禁止提交后续冲突副作用；更换 idempotency key 不能绕过，服务端返回 `RUNTIME_OPERATION_IN_DOUBT`。effectSequence 在 scope 内单调递增，确保 reconcile 后恢复明确顺序。

### ContextWindow

- `contextWindowId`、sessionId、assignmentId、ordinal、runtimeWindowId
- startedByInvocationId、endedByInvocationId
- transitionStrategy：`fresh_window` / `summary_compaction` / `opaque_compaction` / `runtime_native`
- trigger：`manual` / `token_budget` / `model_change` / `resume` / `runtime`
- contextBriefId、contextBriefVersion
- model/config/skill/environment fingerprint
- startedAt、endedAt、status、currentVersion

一个 Session 可以依次包含多个 ContextWindow，但一个 ContextWindow 只服务一个 Assignment。Session 切换 Assignment 时必须关闭当前窗口并创建新窗口，不能沿用旧 Brief、grant 或 History scope。ContextWindow 是模型当前可见上下文的运行时边界，不是 Task、Assignment 或业务事实的权威来源。窗口重置、压缩或丢失不能改变 Task 状态。

### WorkingNote / ContextCheckpoint

WorkingNote 保存 Agent 主动维护的跨窗口工作记忆：

- `noteId`、taskId、assignmentId、sessionId、contextWindowId
- logicalName、scope、currentContentLinkId、contentHash
- sourceEvidenceRefs、provenance、createdBy
- status：`active` / `superseded`
- currentVersion、createdAt、updatedAt

WorkingNote 正文通过 `relationType=content` 的 ArtifactLink 表达。同一 Assignment 内 active WorkingNote 的 logicalName 唯一，重试由 idempotency key 返回原结果。create/append/write 先按 Artifact Blob 协议发布新 Blob；最终 SQLite transaction 创建新的 active content Link，把旧 Link 从 active 改为 superseded 并设置 supersededByLinkId，更新 currentContentLinkId/contentHash/currentVersion，再写 DomainEvent。append/write 必须同时校验 Note expectedVersion 和旧 content Link expectedVersion；旧正文按 retention 策略保留，不再参与普通权限判定。

ContextCheckpoint 记录窗口切换或恢复边界上的可验证恢复点：

- `checkpointId`、sessionId、contextWindowId、assignmentId
- taskVersion、ownerEpoch、nextAction、blockerRefs
- artifactRefs、Git snapshot refs、workingNoteRefs
- contextBriefVersion、trigger
- status：`complete` / `partial` / `failed`
- missingRefs、failureReason、createdAt

WorkingNote 是执行者声明且必须版本化，不能直接修改 Task、ReviewDecision、Requirement、Decision 或 Preference。ContextCheckpoint 优先保存权威事实的引用和版本，不复制出第二套任务状态。

### ContextBrief

- 当前目标、下一步和验收标准；
- 相关 Requirement、Decision 与 Preference 引用；
- Task、Artifact 和 Git 的权威事实引用；
- 按需选择的 History、WorkingNote 和 ContextCheckpoint 引用；
- 上下文生成时间、template/schema 版本；
- Task 版本、owner epoch 和 model/config/skill/environment fingerprint。

每个新 ContextWindow 都从当前权威事实重新生成 ContextBrief。旧窗口摘要和 WorkingNote 只能作为带 provenance 的输入，不能覆盖更新后的 Task 或授权事实。

这些实体扩展 Task 的执行证据，不替代 Task 本身。

## 5. 探索 X：业务事实与实现认知扩展

### BusinessSystem / BusinessScenario

- BusinessSystem：systemId、workspaceId、name、description、currentVersion
- BusinessScenario：scenarioId、systemId、name、description、status（`active` / `retired`）、currentVersion

Scenario 可关联多个 BusinessFact 和 Task，但不以 Repository、Branch 或 Commit 作为身份。

### BusinessFact / FactRevision

- BusinessFact：factId、workspaceId、systemId、factType、stableKey、currentRevisionId、currentVersion
- factType：`requirement` / `business_rule` / `decision` / `glossary` / `domain_entity`
- FactRevision：revisionId、factId、status、sourceRefs、proposedBy、confirmedBy、createdAt、currentVersion
- revision status：`candidate` / `confirmed` / `rejected` / `superseded`

BusinessFact 提供不随 Git 变化的稳定身份，正文通过 `relationType=content` 的 ArtifactLink 绑定 FactRevision。一个 fact 同时最多有一个 current confirmed revision；确认新 revision 时以事务方式 supersede 旧 revision，不删除历史。

ConfirmFactRevision Command 必须携带 expectedFactVersion、expectedCandidateRevisionVersion、expectedCurrentRevisionId，以及存在旧 confirmed revision 时的 expectedCurrentRevisionVersion。taskd 在同一事务中验证这些前置条件，再更新 candidate、BusinessFact.currentRevisionId 和旧 revision；任一不匹配都整体失败并要求刷新。

### Component

- Component：componentId、repositoryId、name、kind、locator、currentVersion

探索 X 在既有 Repository/Worktree 身份之上增加 Component；它们是实现载体，不是 BusinessFact 的身份来源。

### CommitSnapshot / WorktreeSnapshot

- CommitSnapshot：commitSnapshotId、repositoryId、commitSha、treeHash、observedAt、scannerVersion
- WorktreeSnapshot：worktreeSnapshotId、worktreeId、repositoryId、headSha、captureOperationId、stateToken、captureStatus、indexState、observedAt、verifiedAt、scannerVersion
- CaptureOperation：captureOperationId、worktreeId、repositoryId、capturePolicyVersion、commandSchemaVersion、canonicalRequest、requestHash、principalId、actorId、idempotencyKey、correlationId、state、evidenceManifestHash、observationPlanHash、expectedVersions、startStateToken、evidenceStateToken、endStateToken、snapshotId、failureReason、currentVersion、createdAt、completedAt
- capture state：`capturing` / `prepared` / `blobs_published` / `applied` / `conflicted` / `failed` / `cancelled`
- CaptureArtifactIntent：intentId、captureOperationId、artifactId、relationType、ordinal、contentHash、blobHash、size、stagingName、storagePath、intentState、failureReason、currentVersion、createdAt、publishedAt、appliedAt
- capture intentState：`prepared` / `blob_published` / `applied` / `cancelled` / `failed`
- indexManifestHash：按 path、mode、stage、blob 排序的 canonical index manifest hash
- stagedDiffHash、unstagedDiffHash、untrackedManifestHash
- submoduleManifestHash、sparseCheckoutHash、missingEvidence、capturePolicyVersion

CommitSnapshot 只描述确定 commit 的提交树。同一个 headSha 下的不同 index、工作区差异或未跟踪文件必须形成不同 WorktreeSnapshot；不得仅以 `repositoryId + commitSha` 标识未提交现场。扫描若只读取提交内容，结果只能绑定 CommitSnapshot，并明确忽略工作区 Diff。

每个 evidence Artifact 通过 ArtifactLink 绑定 snapshot，relationType 分别为 `index_manifest`、`staged_diff`、`unstaged_diff`、`untracked_manifest`、`submodule_manifest` 或 `sparse_checkout`；hash 用于身份与校验，ArtifactLink 用于长期取回，不允许只保存 hash。canonical index manifest 必须保留 unmerged index 的 stage 1/2/3。Submodule 记录 gitlink SHA 和 dirty 状态；sparse checkout 记录模式与 pattern。`captureStatus` 为 `complete` / `partial`，未捕获的敏感或超限内容写入 missingEvidence；partial snapshot 不能被宣称为完整可复核证据。

WorktreeSnapshot 只能由以下防撕裂、多 Artifact 原子 capture protocol 发布：

1. taskd 先取得 worktree 级内部 capture lease，读取初始现场并计算 startStateToken，再在一个 transaction 中创建 pending CommandReceipt 和 `capturing` 的 durable CaptureOperation，固定 canonicalRequest/requestHash、身份/幂等信息、capturePolicyVersion、纳管范围和全部业务 expectedVersions。随后用同一 canonicalizer 读取完整 index stages、staged/unstaged 内容、untracked/submodule/sparse-checkout 现场，并把证据写入按 operationId 归属的 staging、计算 hash/size；此阶段崩溃不产生业务可见对象，恢复器只可从固定 canonicalRequest 重新执行完整采集或清理归属明确的 staging。
2. 证据 staging 完成后，一个 seal transaction 创建全部 pending Artifact 和完整 CaptureArtifactIntent 清单，固定 relationType/ordinal、预分配 artifactId/intentId、预期 hash/size、evidenceManifestHash 以及可选的 Observation plan/hash，并把 operation 改为 prepared；清单固定后不得追加、删除或替换证据。CaptureArtifactIntent 的临时合法 target 是 internal CaptureOperation，不要求尚未存在的 WorktreeSnapshot。
3. 每个 Blob 按普通协议完成 hash 校验、原子 rename 和目录 fsync，但只把对应 Artifact/CaptureArtifactIntent 记为 pending/blob_published；此时不得 finalize Artifact、创建业务 ArtifactLink、Snapshot、Observation 或业务 DomainEvent，因此逐个 Blob 发布不会暴露半完成 Snapshot。
4. 证据全部发布后，把 operation 标为 blobs_published，从已发布证据自身计算 evidenceStateToken，再重新读取现场得到 endStateToken。只有 `startStateToken = evidenceStateToken = endStateToken`，完整 intent 清单与 evidenceManifestHash/observationPlanHash 匹配，全部 Blob 的 hash/size 有效，且所有 expectedVersions 仍成立时，才进入 publish transaction。
5. 一个 SQLite publish transaction 原子地 finalize 清单中的全部 Artifact、把全部 CaptureArtifactIntent 标为 applied、创建 WorktreeSnapshot 和所有 evidence ArtifactLink，并创建该 operation 的固定 Observation plan 中已通过前置校验的 MappingObservation，最后把 CaptureOperation 标为 applied、完成 CommandReceipt。Snapshot.stateToken 保存相等的 token；事务提交前这些对象一概不可见。
6. 任一 Blob、hash、版本或 token 校验失败时，在同一 transaction 把整个 CaptureOperation 和 CommandReceipt 写入确定性失败终态，并把未被其他 active Link 引用的本次 Artifact 标为 orphaned/failed；不得发布 Snapshot、evidence Link 或 Observation。启动恢复只可依据 sealed 的固定清单继续 Blob 发布或执行同一个原子 publish transaction，不能缩减清单后发布 partial Snapshot；尚未 seal 的 capturing operation 只能按固定 canonicalRequest 重新执行完整采集或取消，不能猜测缺失清单。

`partial` 只表示 capture policy 在清单固定前明确排除且已写入 missingEvidence/排除清单的内容，不能用来掩盖 Blob 失败、并发修改或 token 不一致。内部 lease 只串行化 taskd 自身的 Git 写入；外部编辑器或 Git 进程造成的变化仍必须由 token 校验发现。

### ImplementationReference / ImplementationReferenceRevision

- ImplementationReference：implementationReferenceId、componentId、kind、stableKey、currentVersion
- ImplementationReferenceRevision：referenceRevisionId、implementationReferenceId、snapshotType、snapshotId、locator、signature、createdAt

ImplementationReference 是稳定逻辑身份；路径、API、表或 Symbol locator 只存在于绑定 CommitSnapshot/WorktreeSnapshot 的不可变 Revision。移动或重命名产生新 Revision，不修改历史 locator。

### ImplementationMapping / MappingObservation

- ImplementationMapping：mappingId、factId、implementationReferenceId、confirmationStatus、evidenceRefs、confirmedBy、currentVersion
- confirmationStatus：`candidate` / `confirmed` / `rejected`
- MappingObservation：observationId、mappingId、snapshotType、snapshotId、observedReferenceRevisionId、expectedReferenceRevisionId、expectedRevisionContextRef、contextType、contextKey、contextRevision、freshnessState、evidenceRefs、observedAt、scannerVersion
- contextType：`workspace_baseline` / `repository_ref` / `worktree` / `release_baseline`
- freshnessState：`current` / `stale` / `missing`

V1 的 ImplementationMapping 只表示“某 BusinessFact 在逻辑上由哪个实现引用承载”的稳定关系；Scenario 通过其关联的 BusinessFact 间接获得实现视图。MappingObservation 是绑定具体 snapshot 和观察上下文的不可变观察。taskd 按 contextType 规范化并验证 contextKey/contextRevision：Workspace baseline 使用 workspaceId + baseline manifest hash；Repository ref 使用 repositoryId + 完整 ref name + observed commit SHA；Worktree 使用 worktreeId + worktreeSnapshotId；release baseline 使用 workspaceId + stable release key + baseline manifest hash。baseline manifest 列出参与的 repository/snapshot，Observation 的 snapshot 必须属于对应 contextRevision。

freshness read model 必须以 `(mappingId, contextType, contextKey, contextRevision)` 为查询边界，只在兼容上下文内选择最新有效 observation。没有匹配观察时返回派生状态 `unobserved`；不得用其他 Branch、Worktree 或 release baseline 的较新时间戳覆盖当前视图。

MappingObservation 字段不变量：

- observedReferenceRevisionId 与 expectedReferenceRevisionId 只要非空，其 ImplementationReferenceRevision.implementationReferenceId 都必须等于 `ImplementationMapping.implementationReferenceId`；不能引用另一个 Mapping 所承载的实现对象。
- observedReferenceRevisionId 非空时，其 `snapshotType/snapshotId` 必须与 Observation 的 `snapshotType/snapshotId` 完全一致。
- expectedReferenceRevisionId 非空时 expectedRevisionContextRef 必填，且必须规范化为指定 comparison baseline，或同一 mapping、兼容 context lineage 中的 last-known MappingObservation。baseline 来源要求 Revision 的 snapshot 被该 baseline manifest/ref/worktree 明确选中；last-known 来源要求 expectedReferenceRevisionId 等于被引用 Observation 的 observedReferenceRevisionId。Repository ref 要求同 repository + 完整 ref，Worktree 要求同 worktree，Workspace baseline 要求同 workspace 和显式 baseline manifest，release baseline 要求同 workspace + stable release key；不得从不相关 Branch、Worktree 或 baseline 借用 expected revision。expectedReferenceRevisionId 为空时 expectedRevisionContextRef 也必须为空。
- `current`：observedReferenceRevisionId 必填并精确绑定当前 snapshot；expectedReferenceRevisionId 可空或按上述规则指向比较基线。
- `stale`：observedReferenceRevisionId 必填并精确绑定当前 snapshot，表示目标仍存在但 locator/signature/evidence 不满足基线；expectedReferenceRevisionId 可按上述规则指向期望 revision。
- `missing`：observedReferenceRevisionId 必须为空，expectedReferenceRevisionId 和 expectedRevisionContextRef 必填并指出原本期望找到的 baseline/last-known revision。

### DriftFinding

- `findingId`、workspaceId、factId、mappingId
- baselineSnapshotRef、observedSnapshotRef、evidenceRefs
- classification：`unclassified` / `branch_exception` / `code_issue` / `fact_change`
- status：`open` / `classified` / `resolved` / `dismissed`
- resolutionTaskId、resolutionRevisionId、createdAt、resolvedAt、currentVersion

DriftFinding 只记录差异，不直接修改 BusinessFact 或代码。分类为 code_issue 时可创建 Task；分类为 fact_change 时可创建 FactRevision candidate；branch_exception 保留事实并记录例外。

### Preference

- id、scope、content、sourceRefs
- status：`candidate` / `confirmed` / `rejected` / `superseded`
- confidence、confirmedBy、currentVersion

Preference 的候选 scope 区分 global、workspace、task-type 和 task-local；具体取舍由探索验证。用户明确表达与 AI 推断必须有不同 provenance。

### Feedback / OptimizationProposal

- Feedback 关联 Review、返工、澄清或阻塞事件；
- OptimizationProposal 包含目标版本、证据、差异、风险、评估和回滚计划；
- PromptTemplate 可处于 candidate、canary、live、retired，但 live 变更需要用户确认。

## 6. 关键关系

```text
Workspace 1──N Repository / Task / BusinessSystem
Repository 1──N Worktree
BusinessSystem 1──N BusinessScenario / BusinessFact
BusinessFact 1──N FactRevision
BusinessScenario N──N BusinessFact
Task 1──N Subtask
Task N──N Task                  via TaskRelation
Task N──N Repository / Worktree via TaskContextBinding
Actor 1──N TaskOwnership
Task 1──N TaskOwnership          one active row, ownership authority
Task 1──N DomainEvent / Comment / ReviewSubmission
ReviewSubmission 1──N ReviewSubmissionEvidence / ReviewDecision
Task 1──N TaskCheckpoint       stage A, no Session required
Task 1──N Assignment            optional stage D
Assignment 1──N AgentRun        at most one starting/active
Assignment 1──N RuntimeOperation optional stage D
Session 1──N ContextWindow      optional stage D
Assignment 1──N ContextWindow   one active scope per window
Assignment 1──N WorkingNote / ContextCheckpoint
Repository 1──N Component / CommitSnapshot / WorktreeSnapshot
ImplementationReference 1──N ImplementationReferenceRevision
BusinessFact N──N ImplementationReference  via ImplementationMapping
ImplementationMapping 1──N MappingObservation
Aggregate N──N Artifact          via ArtifactLink
Artifact 1──N ArtifactLink / ArtifactFinalizeIntent / CaptureArtifactIntent
BackupOperation 1──N BackupArtifactPin
CaptureOperation 1──N CaptureArtifactIntent
CaptureOperation 1──0..1 WorktreeSnapshot
Aggregate 1──N DomainEvent
DomainEvent N──N BusinessFact / Preference / Proposal evidence refs
```

## 7. 并发、事务与保留

- 所有可变 aggregate 包含 `currentVersion`，Command 必须带 `expectedVersion`。
- Review accept/request-changes 必须同时校验 Task 与 ReviewSubmission 版本，并在同一事务中写 Decision、Submission 状态、Task 状态及对应事件；重新打开不能复用旧 cycle。
- 跨 aggregate 的 FactRevision 确认必须同时验证 BusinessFact、candidate revision 和旧 confirmed revision 的版本/身份，并在一个事务中提交。
- SQLite transaction 原子提交 aggregate 状态、关系变化和 DomainEvent/outbox envelope。
- Artifact Blob 发布不宣称跨文件系统与 SQLite 的 ACID；单 Artifact 通过 pending metadata、publish-before-reference、幂等 finalize、启动恢复和孤儿 GC 达到可恢复一致性，多 Artifact WorktreeSnapshot 另由 CaptureOperation 固定清单并原子 promotion。
- owner 变更使用 owner epoch；阶段 D 的 AI 长操作再增加 lease 与 heartbeat。
- 事件和 ReviewDecision 默认不可原地改写；更正通过补充事件表达。
- unlink 必须以 expectedVersion 将 ArtifactLink 从 active 变为 unlinked 并记录 unlinkedBy/unlinkedAt/DomainEvent；正文替换使用 superseded。只有不存在任何 active Link，且 superseded/unlinked Link 均已超过保留期时，GC 才可按协议把 Artifact claim 为 deleting 并回收独占 Blob。Artifact 最小元数据和引用状态的保留期限由数据策略决定。
