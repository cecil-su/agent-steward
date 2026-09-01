# 会话数据、隐私与本地存储

> 阶段说明：`0.1` 只要求 Workspace/Repository/Worktree metadata 与本地备份；`0.2` 增加任务历史、评论和附件；Phase 3 提供限定 Assignment scope 的最小加密 History/WorkingNote Store；完整 AI Session Store、跨宿主 importer、Prompt 归档和长期索引在 Phase 6 增量加入。

## 1. 数据目标

AI 与优化阶段默认完整保留用户选择纳管的本地会话，以支持：

- 跨会话恢复；
- 用户意图和偏好分析；
- 重复工作流识别；
- Prompt、流程和 Skill 优化；
- 失败、返工和权限事件审计。

默认不上传遥测或会话数据。

## 2. 数据分层

### SQLite 元数据

- Task/Session/Invocation/Assignment/ContextWindow；
- RoleGrant/Capability/Lease；
- WorkingNote/ContextCheckpoint 元数据和版本；
- Artifact 元数据、hash、provenance 与 ArtifactLink；
- Prompt 版本和 hash；
- Tool call 索引和统计；
- Git plan/operation；
- Review/Test/Optimization 索引。

### 加密 Blob Store

- 完整原始会话；
- 完整 PromptInstance；
- 大型 tool output；
- 报告、截图和附件；
- FactRevision、业务 Wiki 和图的正文；
- WorktreeSnapshot 的 canonical index manifest、staged/unstaged diff、untracked/submodule/sparse-checkout evidence；
- WorkingNote/ContextCheckpoint 正文；
- 导入的宿主 Session 文件。

### 衍生索引

- 全文索引；
- embedding（可选、本地生成）；
- 用户偏好候选；
- 重复流程聚类；
- Prompt 质量指标。

衍生数据必须可从原始数据重建，并记录生成器版本。

### 分阶段存储边界

- Phase 3：保存首个 Runtime 所需的 ContextWindow、最小 Session History、WorkingNote、ContextCheckpoint 和 PromptInstance；正文进入本地加密 Blob，SQLite 保存 scope、版本、hash 和 provenance；只提供限定范围的 list/read/search。
- Phase 6：增加完整会话采集、多宿主 importer、长期全文索引/embedding、跨 Session 检索以及更完整的保留和迁移能力。

Phase 3 的上下文恢复不能依赖 Phase 6 才提供的存储能力；Phase 6 只扩展采集范围、宿主覆盖和索引深度。

Artifact Blob 与 SQLite 不构成一个物理事务。所有阶段共用领域模型定义的 publish-before-reference 协议：pending metadata 和 durable ArtifactFinalizeIntent 不对业务查询可见，只有 Blob 原子发布且 Intent 的请求 hash、版本、权限和 Link 计划仍有效时，才能在 finalize transaction 中创建 active ArtifactLink；冲突 Intent 终止而不永久重试。WorktreeSnapshot 的多 Artifact evidence 由 CaptureOperation 固定完整 intent 清单，全部 Blob 发布并通过 token 校验后才在一个事务中原子 promotion，不能逐项暴露。V1 每个 Artifact 独占物理 Blob，启动恢复和 GC 按 Artifact 状态处理。

### 备份一致性与密钥恢复

备份不是“先复制 SQLite、再尽力复制 Blob”。taskd 必须按领域模型的 BackupOperation 协议，在数据库层短期 writer gate 内从一个 SQLite 一致性点固定 eventWatermark、Artifact manifest 和 active BackupArtifactPin，再生成 online backup；该 gate 覆盖 Application Command、Artifact/Runtime recovery、lease/heartbeat、event ack/retention、GC、backup/recovery、migration 和所有维护 writer。Blob 复制期间 pin 阻止 GC 把 manifest 中的 Artifact claim 为 deleting。只有数据库、manifest、全部 Blob hash 和 key envelope 均验证通过并写入最终完整性 manifest 后，备份目录才可原子发布；启动 reconcile 负责把已发布但 live operation 未 complete 的有效备份补记完成并释放 pin。

可移植备份包含加密 key envelope，不包含明文数据密钥：默认由用户提供的备份口令/恢复密钥包装，或由显式配置且 restore 环境可访问的外部 key provider 包装。仅保存在原设备 OS Keychain 的引用不能作为可移植恢复材料；若用户选择 device-bound backup，CLI/UI 必须明确标为不可跨设备恢复。restore 先在隔离临时根验证 envelope 可解包、数据库 `quick_check`/schema、所有保留中的 Link → Artifact → Blob hash，并终止/隔离备份时刻的 pending Artifact、Runtime、AgentRun、lease/capability 和 Git 外部 operation，禁止在新机器自动重放。restore 的幂等 receipt、manifest hash、old/new data-root generation 与切换阶段写入两个 data root 之外的受保护 bootstrap journal，再于 taskd 独占维护模式原子启用整套数据；任何一项失败都不得覆盖 live store。

## 3. Provenance

所有消息和内容必须标记来源：

```text
user
assistant
system
developer
tool
repository
external-web
runtime
subagent
```

只有明确的 `user` 内容可以直接作为用户意图证据。repository、web 和 tool 内容可能包含 Prompt Injection，不得自动转为长期偏好、规则或 Skill。

## 4. 上下文连续性与权威分层

```text
Task / Confirmed Business Fact   正式权威
ContextBrief                     按窗口生成的派生输入
ContextCheckpoint                引用权威版本的恢复点
WorkingNote                      Agent 可写、版本化的工作记忆
Session History                  对“发生过什么”的保留期内不可改写证据
```

- Session History 在保留期间不可原地改写，证明某段内容曾出现，但不代表内容本身正确或已经获得用户确认。
- WorkingNote 是执行者声明，必须带 provenance、scope 和版本；不得自动提升为 Task 状态、业务事实或用户偏好。
- ContextCheckpoint 保存 Task 版本、owner epoch、Artifact/Git 引用和恢复所需最小状态，不复制出第二套权威数据。
- 每个新 ContextWindow 从当前 Task 与已确认事实重新生成 ContextBrief；旧 History 和 WorkingNote 只按需读取。
- History 查询默认只读，WorkingNote 先按 Assignment + logicalName 幂等创建，再使用受控 ArtifactLink 写入正文，不暴露任意文件写入。
- 本地实现优先使用 SQLite 元数据、加密 Blob 和本地索引；任何远程 history/notes 后端都需要单独、可见、可撤销的授权。
- History/notes 工具必须由服务端绑定 principal、session 和 assignment，并对参数、结果大小、截断和审计设置边界。

## 5. 用户意图分层

### Confirmed Preference

用户明确确认的长期偏好。

### Candidate Preference

重复出现但未确认的行为模式，需要向用户确认。

### Task-local Context

只属于当前任务，不可自动提升为长期偏好。

每条偏好保留 evidence message IDs、创建方式和用户确认记录。

## 6. 隐私与加密

- 数据默认仅绑定本机用户。
- Blob Store 默认加密；密钥由 OS Keychain 或用户口令保护。
- 备份使用独立的加密 key envelope 恢复数据密钥；明文密钥和仅对原设备有效的 Keychain handle 不写入可移植备份。
- 数据库至少保护敏感字段；是否采用 SQLCipher 属于待决策项。
- Secret/token/cookie 检测用于降低意外暴露，但不应破坏原始审计能力。
- WorktreeSnapshot 可能包含未提交源码和 secret；capture policy 必须限制路径、大小和内容类型，正文进入加密 Blob，未采集项记录为 missing evidence。
- Artifact 不继承单一 Task 的权限；普通读取、导出或删除必须通过 active ArtifactLink 的 aggregate scope 授权，不能仅凭 artifactId 或 superseded/unlinked Link 访问 Blob；历史取证另需 retention/audit capability。
- ReviewSubmission 在 submit transaction 中为证据建立自己的 active `review_evidence` Link；普通读取按 Submission → Task scope 授权，source Link 后续 supersede/unlink 不得缩短证据保留或使待验收证据不可读。
- Optimizer 默认读取聚合与脱敏数据；读取完整 Session 需要额外授权。

## 7. 用户控制

必须提供：

```text
data status
data export
data inspect
data delete-session
data delete-task
data retention
data reindex
data telemetry-status
```

删除操作应说明：

- 先把删除范围内的 active ArtifactLink 以版本化操作改为 unlinked；如果同一 Artifact 仍被 FactRevision、Snapshot、其他 Task/Session 或任何保留中的 Link 引用，不得删除原始 Blob；
- 只有不存在 active Link，且所有 superseded/unlinked Link 均已超过 retention 后，GC 才能以 currentVersion 将 Artifact 原子改为 deleting；新 Link 只允许 finalized Artifact，因此 deleting claim 提交后不能再关联；
- 删除独占 Blob 后把 Artifact 改为 deleted；若崩溃在 deleting 中，启动恢复按 deletionOperationId 幂等完成，而不是重新开放 Link；
- 删除或重建衍生索引；
- 以受审计的 retention/delete operation 写入 tombstone，不原地伪造或改写既有 History；
- 审计日志中保留对象 ID、删除范围、操作者和时间等最小删除事实，不保留已删除正文。

## 8. 保留策略

对用户选择纳管的会话，默认保留策略仍需支持：

- 永久保留；
- 按 Task/Workspace 保留；
- 按时间清理；
- 用户标记不可删除；
- 敏感 Session 单独删除。

## 9. 无遥测承诺

公开版本默认：

- 不发送使用统计；
- 不上传 crash log；
- 不上传 Prompt/Session；
- 不调用远程 embedding；
- 任何联网分析都需要显式启用并展示目标、范围和数据类型。
