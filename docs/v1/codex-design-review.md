# Agent Steward V1 设计审阅（Codex）

> 历史评审：正文保留当时的范围与建议供追溯，不作为当前交付清单。最新任务接续主线见 01/02/11 篇；本轮跟踪的 Review、restore generation、Runtime 重试目标与事实版本问题见 D-020–D-023；Review 撤回设计已冻结，仍待实现验证。规划调整不代表旧技术问题均已解决。


状态：**待设计阶段处理**  
审阅范围：`docs/v1` 全部 V1 设计文档  
目的：记录编码前需要澄清或补全的设计问题，并给出建议的处理方向。

## 总体结论

当前设计在单一权威、最小授权、Git 事实绑定、Runtime 与业务状态分离，以及 Standard/Hardened 分级方面方向合理。

现阶段的主要问题不是设计原则缺失，而是部分原则尚未转化为可实现、可恢复、可测试的明确契约。建议先处理 V1 范围、状态机、安全模式承诺和事务恢复协议，再冻结实现基线。

## P0：编码前应解决

### 1. 缩小并明确 V1 的可交付闭环

#### 问题

当前路线同时覆盖任务系统、双 Runtime、Git 治理、Hardened 隔离、完整会话存储和自身优化，实际包含多个可独立交付的产品阶段。路线图虽然提出采用垂直切片，但现有 Phase 仍以横向能力建设为主，较晚才能形成端到端用户价值闭环。

#### 建议

定义一个明确的 V1-MVP 闭环：

```text
创建任务
→ 授予 Task Owner
→ 派发 Assignment
→ 一个 Runtime 执行
→ 提交证据
→ 人工验收
→ 本地 commit plan/execute/verify
```

- 首个 Runtime 完成闭环后，再通过同一 conformance contract 接入第二个 Runtime。
- 双 Runtime 支持可以继续作为 V1.0 的目标，但不应阻塞首个可用切片。
- Optimizer L3–L4 建议移出 V1-MVP，作为后续独立里程碑。
- 为 V1-MVP 编写单独、可自动验证的退出条件。

### 2. 正式定义所有状态机

#### 问题

需求要求状态转换经过状态机，但尚未完整列出 Task 的状态、合法转换和转换条件。设计同时强调 Agent 完成、业务验收和 Git 集成完成是不同事实，如果没有正式状态机，实现时容易把这些概念重新混合。

#### 建议

增加正式状态机规范，至少覆盖：

- Task；
- Assignment；
- RoleGrant；
- Lease；
- GitPlan；
- GitOperation。

每个转换需要明确：

- 允许发起转换的 principal/role；
- 当前状态与目标状态；
- `expectedVersion`、owner epoch、lease 等前置条件；
- 写入的领域事实和 outbox event；
- 失败、超时和崩溃后的状态；
- 是否允许重试，以及重试的幂等规则。

特别需要定义：

- `Assignment.completed` 仅表示 Agent 已提交结果和证据；
- 业务验收应使用独立状态或事实；
- Git 集成完成应由已验证的 GitOperation 表示；
- Task 何时可以进入最终完成状态。

### 3. 分离 Standard 与 Hardened 的安全承诺

#### 问题

安全设计已经承认：当 Agent 与用户共享 OS 身份、数据库和 Git 凭据时，无法形成硬安全边界。但部分路线图退出条件仍要求 Agent 不能绕过 taskd，这在 Standard 模式下无法被真正保证。

另外，`Local Authenticated RPC` 尚未定义身份引导和认证细节，Human/Admin principal、Agent capability 和 Runtime Adapter 身份如何建立仍不明确。

#### 建议

建立“威胁—运行模式—安全保证—验收测试”矩阵：

| 模式 | 可承诺的保证 | 不应承诺的保证 |
|---|---|---|
| Standard | 防误操作、权限一致性、受管流程审计、事实漂移检测 | 阻止共享身份的 Agent 直接读文件、调用原生 Git 或访问用户凭据 |
| Hardened | 通过独立身份、ACL、沙箱和凭据隔离阻止绕过 | 抵御管理员、恶意软件或操作系统攻破 |

同时补充：

- taskd 首次启动和 Human/Admin principal 引导；
- 本地 RPC 的传输方式、socket/pipe ACL 和服务端身份验证；
- capability 的签发、注入、存储、撤销和过期；
- 如何避免 capability 出现在 Prompt、命令行、环境转储和普通日志中；
- Human approval channel 如何证明请求来自可信用户，而不是 Agent shell；
- Standard 与 Hardened 各自独立的验收条件。

### 4. 明确审计与业务写入的原子性

#### 问题

当前请求流程描述为：在事务内更新状态和 outbox event，随后记录审计事实。如果审计记录不在同一事务中，崩溃可能产生“业务已提交但审计缺失”的窗口。

此外，普通 hash chain 只能帮助检测篡改；如果同一主体能够重写全部日志，它也可以重算未加密的 hash chain。因此，hash chain 本身不等于 append-only 或不可删除。

#### 建议

明确采用以下方案之一：

1. 领域状态、outbox event 和最小审计记录在同一个 SQLite 事务中提交；或
2. 使用有正式恢复算法和故障测试的持久化双写协议。

同时定义：

- 审计写入失败时业务事务是否必须失败；
- hash chain 的链首、分段、轮换和校验方式；
- 是否使用 HMAC/签名，以及密钥由谁持有；
- 是否将链摘要周期性写入受保护的外部锚点；
- Standard 模式只承诺篡改检测，Hardened 模式如何通过 ACL 防止 Agent 改写；
- 审计恢复、导出和完整性检查命令。

### 5. 重构事件投递与幂等模型

#### 问题

单个 Event 上的 `pending/delivered/processed` 状态不足以表达：

- 多个独立消费者；
- 同一消费者的重复投递；
- 业务处理成功但 ack 丢失；
- 投递尝试失败和退避；
- 消费者重建进度。

当前幂等要求也尚未定义 key 的作用域、请求内容校验和保留期限。

#### 建议

将事件模型拆为：

- `DomainEvent`：事务内产生的不可变业务事实；
- `OutboxMessage`：面向目标消费者或主题的待投递记录；
- `DeliveryAttempt`：每次投递及其结果；
- `ConsumerInbox`/`Ack`：消费者的去重与处理确认。

幂等契约至少定义：

- key 的作用域，例如 `(principal, operation, resource scope, idempotencyKey)`；
- 同一 key 对应的规范化 request hash；
- 相同 key、不同 payload 必须返回冲突错误；
- 幂等记录的保留期限和清理条件；
- 处理中崩溃、响应丢失及结果重放时的行为；
- 外部 Runtime/Git 操作如何关联同一个 operation ID。

### 6. 将最小 Blob/Artifact Store 提前

#### 问题

需求规定完整会话和大型 Artifact 使用加密 Blob Store，但路线图直到 Phase 6 才实现完整 Session Store 和 encrypted blob。Phase 2–4 的报告、测试证据、Git stdout/stderr 和 checkpoint 已经依赖 Artifact 持久化。

默认完整保留会话还可能保存 token、cookie、私有源码和用户粘贴的外部内容。仅凭消息来源是 `user`，不能证明其中内容就是用户希望长期固化的偏好。

#### 建议

- 在 Phase 1/2 实现最小加密 Artifact Store，支持报告、证据和 Git 操作输出。
- 在 Phase 6 增加完整会话采集、全文索引、embedding 和分析能力。
- 定义 metadata-only、redacted 和 full-capture 等采集级别。
- 将敏感正文与普通审计事实分离；审计可保存 hash、类型和时间，不必保存 secret 正文。
- 定义 OS Keychain/用户口令的解锁、密钥轮换、备份恢复和密钥丢失行为。
- 明确全文索引、embedding、临时文件和备份是否加密。
- 说明 SSD、备份和 SQLite page reuse 下“安全删除”的实际保证边界。
- 用户消息只能形成 Candidate Preference；升级为 Confirmed Preference 仍需显式确认。

## P1：进入对应模块前解决

### 7. 收紧 GitPlan 的规范化和失效规则

#### 问题

“任何事实差异都使计划失效”可能把与操作结果无关的变化也视为失效，导致计划频繁重建。`hooks/config/credential context` 的含义也不够明确，如果处理不当可能把敏感凭据带入 plan 或审计。

权限策略中的 `allow-once/allow-task/allow-repo/allow-global` 与用户对具体 `planHash` 的批准，也需要区分为不同概念。

#### 建议

为 commit、merge、push 分别定义 canonical input snapshot：

- 哪些字段参与 `planHash`；
- 哪些变化必然使 plan 失效；
- 哪些变化只需重新记录，不影响执行；
- 时间戳、锁文件等瞬态数据如何处理；
- credential 只保存 broker identity/reference/hash，不保存 secret；
- pre-existing index 和用户未暂存修改如何保护；
- hook 修改文件或 index 后为何进入 `needs_reconciliation`；
- 精确暂存是否使用临时 index 或其他隔离机制。

建议将以下概念分开：

- `GitPolicy`：某类操作默认 deny/ask/allow；
- `ApprovalGrant`：用户对具体 planHash 或明确范围的授权；
- `GitPlan`：冻结的输入和预期结果；
- `GitOperation`：一次实际执行及验证结果。

### 8. 增强 Runtime Adapter 的故障恢复契约

#### 问题

现有 Runtime 接口覆盖正常的 spawn/resume/send/status/close，但不足以处理以下故障：

- Runtime 已成功 spawn，但 taskd 在保存 handle 前崩溃；
- 请求成功但响应丢失，客户端重复调用；
- Host Plugin 重启后丢失内存状态；
- Runtime 长时间 unknown；
- close、cancel、detach 的语义不同；
- 完成事件重复、乱序或晚到。

#### 建议

Runtime contract 增加：

- request/idempotency key；
- 稳定的 external operation ID 和 RuntimeHandle；
- heartbeat、lastObservedAt 和 lease 信息；
- 可重放事件或 event cursor；
- `cancel`、`close`、`detach` 的独立定义；
- adapter 错误到领域错误的标准映射；
- taskd 重启后的 discovery/reconcile 操作；
- 对不支持 resume、status 或 event replay 的能力降级规则。

Conformance tests 应加入上述崩溃窗口和重复请求场景，而不只验证正常生命周期。

### 9. 将 CLI/MCP 契约变为机器可验证产物

#### 问题

目前已有候选命令、工具名和错误结构，但尚未定义正式、版本化的接口描述。CLI 与 MCP 如果分别手写 schema，后续容易发生字段、错误码和授权语义漂移。

#### 建议

- 使用一份版本化 JSON Schema、IDL 或等价契约生成/校验 CLI JSON 与 MCP 输入输出。
- 为所有写操作统一定义 `expectedVersion` 和 `idempotencyKey` 的位置与语义。
- 定义 ID、时间、枚举、null/缺失字段的表示方式。
- 定义分页、排序、过滤及游标稳定性。
- 定义 CLI stdout/stderr、JSON mode 和 exit code 的映射。
- 错误码保持机器兼容，message 只用于人类阅读。
- 增加 protocol/schema 版本协商、字段废弃和数据库迁移策略。
- 对 CLI 与 MCP 执行共享的契约测试和授权测试。

### 10. 增加可量化的非功能验收与故障测试

#### 问题

当前验收原则方向正确，但“完整记录”“相同权限判定”“能够恢复”等表述仍需转化为具体测试。并发、数据规模、启动时间、恢复时限和存储增长也没有明确预算。

#### 建议

增加以下测试类别：

- 状态机与授权矩阵的表驱动/属性测试；
- SQLite commit、outbox、audit、Runtime、Git 各边界的 fault injection；
- 重复调用、响应丢失、时钟变化和 lease 过期测试；
- Windows junction、大小写、UNC path 和 Git worktree 边界测试；
- schema migration、备份恢复和降级兼容测试；
- Standard/Hardened 独立的安全验收套件；
- 最大任务数、事件数、Artifact 容量和索引重建时间的性能预算。

## 建议新增的设计产物

建议在冻结实现前补充：

1. `13-state-machines.md`：所有 aggregate 的状态、转换和不变量；
2. `14-authentication-and-mode-guarantees.md`：身份引导、RPC 认证以及 Standard/Hardened 保证矩阵；
3. `15-events-idempotency-and-recovery.md`：事务、事件、幂等、崩溃窗口和协调算法；
4. `16-v1-mvp-and-acceptance-tests.md`：首个端到端切片及可执行验收条件。

## 建议处理顺序

1. 确定 V1-MVP 范围和端到端退出条件；
2. 冻结 Task/Assignment/Git 等核心状态机；
3. 冻结 Standard/Hardened 的承诺和本地认证机制；
4. 冻结事务、审计、事件、幂等与恢复协议；
5. 提前实现最小加密 Artifact Store；
6. 再决定具体技术栈、首个 Native Host 和后续 Git/Optimizer 细节。

前四项是当前真正阻断设计冻结和编码的部分。其余问题可在对应垂直切片开始前关闭。

## 增量审阅记录：版本绑定、备份与同步一致性

状态：**已纳入规范设计，待实现与故障测试验证**

### 高优先级

1. **ReviewDecision 未绑定被验收版本**：增加 ReviewSubmission、reviewCycle、submittedTaskVersion、acceptanceCriteriaHash 和版本化 evidence set；accept/request-changes 同时校验 Task/Submission 版本，并在一个事务写 Decision 与状态转换。Done 重新打开后必须创建新 cycle，旧 Decision 不可复用。
2. **SQLite 与 Blob 缺少一致性备份协议**：增加 BackupOperation/BackupArtifactPin、短期 write barrier、单一 SQLite 一致性点、Artifact manifest/eventWatermark、online backup、Blob hash 校验和最终完整性 manifest。restore 在隔离根校验 DB/schema/Link → Blob/key envelope 后原子启用。

### 中优先级

3. **Snapshot 与 event cursor 可能跳过事件**：冻结全局 streamPosition；snapshot + eventWatermark 来自同一个 SQLite read transaction，cursor 超出 retention 时返回 CURSOR_EXPIRED。ArtifactBlobPending 改为 internal/audit-only 存储记录，不进入业务 Event Stream。
4. **幂等 Receipt 可能形成授权旁路**：Receipt 命中只阻止重复副作用；返回历史 payload/resultRef 前必须重新验证当前 connection、grant 和对象读取权限。
5. **send-prompt 的 Runtime 目标不明确**：V1 约束每个 Assignment 最多一个 starting/active AgentRun，数据库强制唯一；sendPrompt/status/close 解析并校验该 active Run，不能按时间猜测。
6. **WorktreeSnapshot 可能发生撕裂采集**：发布前比较 startStateToken、evidenceStateToken 和 endStateToken；不一致时有界重试或失败，不得以 partial 发布多个时刻拼接的现场。
