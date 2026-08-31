# Agent Steward V1 设计评审

## 1. 评审范围

本次评审覆盖：

- 根目录 `README.md`；
- `docs/v1/README.md`；
- `docs/v1/01-product-positioning.md` 至 `docs/v1/12-open-decisions.md`。

项目当前仍处于设计阶段。本评审关注设计的一致性、安全边界、可实现性、崩溃恢复、数据隐私和 V1 范围，不涉及具体技术栈选型或代码实现。

## 2. 总体评价

当前设计已经具备较好的基础，尤其包括：

- CLI 和 MCP 共用可信核心、状态机和权限判定；
- Runtime 状态、Agent 完成和任务验收互不等同；
- Git 生命周期采用 Plan → Approve → Execute → Verify；
- 明确区分 Standard 与 Hardened 安全模式；
- Optimizer 默认只能观察和提出候选，不能静默修改自身；
- SQLite、Git、Runtime、Artifact 和审计证据之间已有初步边界。

当前主要风险不是缺少功能，而是部分核心安全协议仍未闭合，同时 V1 范围较大。以下问题建议在接口和实现冻结前处理。

## 3. P0：编码前必须处理的问题

### 3.1 Standard 与 Hardened 的安全保证仍不够明确

#### 问题

Standard 模式下，如果 Agent 与用户使用同一 OS 身份并拥有普通项目 shell 权限，Agent 可能直接读取或修改数据库、调用原生 Git、访问凭据或绕过 taskd。此时工具只能提供流程约束、状态一致性和审计，不能形成对抗性安全边界。

#### 风险

如果产品文档没有明确区分两种模式，用户可能误以为 Standard 模式也能阻止受到 Prompt Injection 的 Agent 绕过控制平面。

#### 建议

增加明确的安全保证矩阵：

- **Standard**：提供一致性、审计、防误操作和协作治理；仅对通过 Steward 执行的操作形成约束，不保证恶意 Agent 无法绕过。
- **Hardened**：通过独立身份、ACL、沙箱、受保护凭据和 Git metadata 隔离，提供可强制执行的安全边界。

同时明确每项 Git 写操作在哪种模式下可以宣称“受控”，并将 Hardened capability probe 提前到首个写操作之前。

涉及文档：`03-architecture.md`、`04-security-model.md`、`07-git-governance.md`、`11-roadmap.md`。

### 3.2 身份建立、Capability 注入和可信审批协议未闭合

#### 问题

当前设计描述了 Principal、RoleGrant 和 Capability 的属性，但没有完整定义：

- Human/Admin principal 如何安全创建和恢复；
- AI Session 如何由宿主可信绑定；
- MCP connection 如何防止调用者伪造 Session、Invocation 或 Assignment；
- taskd 如何区分同一 OS 用户下的人类 CLI 与 Agent CLI；
- capability 如何安全传递、轮换、撤销和防止重放；
- Registry Main 的首个 grant 如何启动。

此外，候选命令：

```bash
stewardctl session attach --grant <one-time-code>
```

与“capability 不出现在普通命令参数或日志中”的原则冲突。

#### 风险

如果身份启动链不可信，后续 RoleGrant、owner epoch 和审批机制都可能被绕过。

#### 建议

补充端到端身份启动协议，至少定义：

- Local RPC 的 peer identity 校验；
- taskd challenge/nonce 和防重放机制；
- capability 的 audience、scope、TTL、使用次数和撤销方式；
- capability 通过受保护管道、继承句柄、stdin 或宿主插件注入，不通过普通参数、Prompt 或可继承环境变量传递；
- Human approval 使用 Agent 无法伪造的渠道；
- 本地 Web UI 如被采用，必须处理认证、CSRF、来源校验和 Agent 自动访问风险。

建议将本问题加入 `12-open-decisions.md`，并提高到与 taskd 运行身份和审批渠道相同的优先级。

### 3.3 状态机尚未形成可执行规范

#### 问题

文档列出了多个状态名称，但缺少完整的状态转换矩阵。目前尚不清楚每个转换的允许角色、版本前置条件、epoch、lease、事务副作用和恢复动作。

需要覆盖的实体至少包括：

- Task；
- Assignment；
- RoleGrant；
- Lease；
- Event；
- GitPlan；
- GitOperation；
- OptimizationProposal/Experiment。

尤其需要定义：

- lease 到期但 Runtime 为 `unknown` 时如何接管；
- owner 转移后旧 Invocation 如何被 fencing；
- `needs_reconciliation` 的进入和退出条件；
- Assignment completed 后任务为何仍可能未验收；
- 跨 aggregate 操作如何保证一致性；
- 重复请求、迟到事件和并发 owner 操作如何处理。

#### 风险

如果直接从文字描述进入编码，不同模块可能产生不一致的状态语义，随后很难通过补丁修复。

#### 建议

为每个可变 aggregate 建立状态转换表，明确：

- 当前状态和目标状态；
- 允许的 principal/role；
- 必需的 `expectedVersion`、owner epoch 和 lease；
- 事务内写入；
- outbox/audit 副作用；
- 幂等行为；
- 失败和 reconciliation 路径。

状态表应能够直接生成或驱动确定性测试。

涉及文档：`03-architecture.md`、`05-domain-model.md`、`06-cli-and-mcp.md`。

### 3.4 Git Executor 的执行级安全设计仍需细化

#### 问题

`07-git-governance.md` 已定义计划和事实快照，但尚未覆盖 Git 自身的大量执行面：

- hooks、clean/smudge filter、external diff、credential helper；
- config include、`core.sshCommand`、submodule 和自定义协议；
- symlink/junction 和 Worktree `.git` 指针；
- 重新校验事实与实际执行之间的 TOCTOU；
- 外部进程同时修改 index、ref 或工作区；
- commit、merge、push 对事实快照的要求并不相同。

“任何事实变化都使计划失效”也可能过于宽泛。如果无关字段变化也导致失效，实际使用会非常脆弱。

#### 风险

受信任 taskd 调用 Git 时，可能被仓库配置或 hooks 引导执行非预期程序；计划校验也可能在执行瞬间被并发修改绕过。

#### 建议

- taskd 使用固定 argv 调用 Git，不经过 shell；
- 清理环境变量并显式控制允许的 Git 配置；
- 对 hooks、filters、submodule、credential helper 和自定义协议建立仓库信任策略；
- commit 优先使用临时 index 或 Git plumbing 精确构造 tree；
- ref 更新使用 compare-and-swap 语义；
- 执行期间持有 Git operation lease 和 Writer fencing；
- 区分“影响操作结果的事实”和无关事实；
- 为 commit、merge、push 分别定义独立 Plan schema、校验规则和恢复策略；
- 明确 hook 修改文件、签名、提交时间和凭据交互对计划确定性的影响。

### 3.5 SQLite、Blob、Outbox、Audit 和 Git 操作的崩溃一致性不明确

#### 问题

当前请求流程提到事务、outbox 和审计，但没有说明 SQLite、加密 Blob 和外部 Git 副作用之间的提交协议。

需要回答：

- Blob 是先写入还是在数据库事务后写入；
- 数据库提交失败时如何清理孤儿 Blob；
- Blob 写入成功但进程崩溃时如何恢复；
- Audit 是否与业务状态和 outbox 同事务；
- Git 已执行成功但 operation 尚未落库时如何识别；
- 重启后如何证明一个 commit、merge 或 push 属于哪个 operation；
- hash chain 如何保存和校验检查点。

#### 风险

仅靠 operation 状态无法覆盖所有崩溃窗口，可能产生状态显示失败但 Git 已执行、Artifact 丢失或审计链断裂等问题。

#### 建议

定义明确的持久化协议：

1. Blob 写入临时位置并完成加密、hash 和 fsync；
2. SQLite 事务原子写入业务状态、Artifact 元数据、outbox 和 audit record；
3. Blob 原子发布，或通过可恢复状态标记完成；
4. 后台清理无法被数据库引用的临时/孤儿 Blob；
5. 外部 Git 操作采用 write-ahead intent，并设计操作级 reconciliation fingerprint；
6. 审计 hash chain 定期生成受保护检查点。

同时明确：hash chain 只能检测篡改；在 Standard 模式下不能保证同身份 Agent 无法删除整个审计文件。

## 4. P1：接口冻结前建议处理的问题

### 4.1 长期 Policy 与单次 Approval 的语义需要拆分

#### 问题

`deny`、`ask`、`allow-once`、`allow-task`、`allow-repo`、`allow-global` 同时承担策略和批准两种职责，而设计又要求批准绑定具体 `planHash`。

#### 建议

拆分为：

- **Policy**：决定某类操作是禁止、必须询问还是允许自动批准；
- **Approval**：绑定具体 plan、事实快照、有效期和使用次数；
- **Policy ceiling**：高层 deny 不能被低层 grant 或 task policy 覆盖；
- **Policy version**：GitPlan 必须绑定生成时的 policy version。

即使策略允许自动批准，也仍应生成、验证并审计具体 plan。

### 4.2 Event 和幂等模型不支持多消费者

#### 问题

单一 `deliveryState: pending/delivered/processed` 无法表示多个消费者各自的投递和 ack 状态。

#### 建议

增加：

- consumer/subscription；
- 每个 consumer 的 delivery、ack 和 processing 状态；
- delivery attempt、lastError 和 dead-letter；
- inbox/outbox 区分；
- idempotency key 的作用域、唯一约束和保留期限；
- Event、AuditEvent 和领域状态转换之间的明确边界。

### 4.3 默认完整保留会话的数据风险较高

#### 问题

本地优先不等于数据天然安全。完整 Session、工具输出、网页内容和附件很可能包含密钥、个人信息或业务敏感信息。

#### 建议

- 首次启用完整采集时明确告知并确认；
- 支持 Session、Task 和目录级排除规则；
- 在采集入口进行 secret 分类、标记或可配置脱敏；
- 使用 Session/Task 级数据密钥，以支持 cryptographic erasure；
- 明确全文索引和 embedding 的加密、重建与删除规则；
- 定义删除证据后 Candidate/Confirmed Preference 和 OptimizationProposal 的失效规则；
- 将审计所需的最小事实与完整会话正文分离。

涉及文档：`09-session-data-and-privacy.md`、`10-self-optimization.md`、`12-open-decisions.md`。

### 4.4 Runtime Adapter 不应直接进入可信核心进程

#### 问题

第三方 Adapter 如果以插件形式加载到 taskd 进程内，将获得与可信核心相同的权限，插件边界会失效。

#### 建议

- Adapter 默认作为独立进程运行；
- 使用窄化、本地认证 RPC；
- Adapter manifest 声明能力、版本和权限；
- taskd 对 Adapter 返回的 Runtime 状态按外部事实处理，不直接视为业务结论；
- 对 Adapter 进行版本固定、来源校验和可选签名；
- conformance tests 增加恶意输出、重复事件、迟到事件、伪造 Session ID 和崩溃恢复测试。

涉及文档：`08-runtime-adapters.md`、`12-open-decisions.md`。

### 4.5 V1 范围过大

#### 问题

当前 Phase 0–7 同时包含：

- 控制平面；
- 权限和状态机；
- 多 Runtime；
- Git commit/merge/push；
- Hardened 沙箱和凭据隔离；
- 完整 Session Store；
- Optimizer L0–L4；
- Prompt、Skill 和 Policy registry。

这实际上包含多个可以独立成产品版本的系统。

#### 风险

过大的 V1 会推迟第一条可验证垂直切片，也容易使安全核心与后期优化功能相互牵制。

#### 建议

将版本范围收缩为：

- **V1 Core**：taskd、SQLite、状态机、角色、审计、CLI/MCP、一个 Runtime、只读 Git；
- **V1 Security**：Hardened 本地 stage/commit/merge；
- **后续版本**：push、多宿主全量 Session、Optimizer L3–L4。

`steward-ui` 虽可暂不实现完整管理功能，但可信审批所需的最小 UI/可信交互入口不能推迟到 Git 写操作之后。

### 4.6 CLI/MCP 契约还需补充版本和通用请求信封

#### 建议

所有写请求使用统一结构，至少包含：

- command/request ID；
- idempotency key；
- correlation/causation ID；
- expected entity version；
- owner epoch；
- policy/grant version；
- schema/API version。

同时定义分页、敏感字段脱敏、错误兼容性和旧客户端行为，确保 CLI 与 MCP 能通过同一契约测试。

## 5. 文档中的具体不一致或歧义

### 5.1 “SQLite 唯一权威”与外部事实权威表述冲突

建议统一为：

> SQLite 是唯一的控制平面状态权威；Git 和 Runtime 是外部事实来源，其采样结果以版本化快照进入控制平面。

这样可以避免把 SQLite 误解为 Git commit、分支或进程状态本身的最终权威。

### 5.2 RoleGrant 的 subject 类型不一致

`04-security-model.md` 示例使用 `subjectSessionId`，但领域模型允许 subject 为 Human、AI Session、Service 或 Runtime Adapter。

建议改为：

- `subjectPrincipalId`：必需；
- `boundSessionId`：可选；
- `boundInvocationId`：可选；
- connection/assignment 绑定由 capability 进一步收窄。

### 5.3 `steward-ui` 的实施时间与可信审批要求冲突

根文档把 `steward-ui` 描述为可后续实现，但 Git 写操作需要 Agent 无法伪造的审批渠道。应在第一个 Git execute 之前提供最小可信审批入口，或明确 Phase 4 之前只允许不需要人类批准的受限实验模式。

### 5.4 “Append-only Audit”在 Standard 模式下不是硬保证

建议表述为：

- 应用层只允许追加；
- hash chain 用于检测内容篡改；
- Hardened 模式通过受保护身份和 ACL 防止 Agent 修改或删除；
- Standard 模式不承诺抵御同 OS 身份下的直接文件操作。

### 5.5 Manual attach 的命令参数会暴露一次性凭据

`--grant <one-time-code>` 可能出现在 shell history、进程列表、日志或 Agent transcript 中。应使用受保护输入通道，并使 code 绑定目标 Session、Assignment、调用方和短有效期。

## 6. 建议新增或提升优先级的设计决策

除现有 D-001 至 D-012 外，建议增加：

1. **Local RPC 与身份启动协议**：transport、peer identity、capability 注入、防重放和撤销。
2. **状态机与 fencing 规范**：owner epoch、lease expiry、人工接管和 reconciliation。
3. **跨存储崩溃一致性协议**：SQLite、Blob、outbox、audit 和外部 Git 副作用。
4. **Git 执行环境信任策略**：hooks、config、filters、submodule、凭据和仓库信任。
5. **Adapter 隔离与插件供应链边界**：进程模型、签名、兼容性和最小权限。

建议的决策优先顺序：

1. taskd 运行身份与 Standard/Hardened 保证；
2. Local RPC、身份启动和可信审批；
3. 状态机、lease 和 fencing；
4. 数据加密与崩溃一致性；
5. Git Executor 安全模型；
6. 技术栈；
7. 首个 Native Host。

## 7. 建议的下一步

在开始搭建 taskd 之前，优先产出以下三份可验证规格：

1. **Standard/Hardened 保证矩阵与身份启动协议**；
2. **Task、Assignment、Lease、Event 和 GitOperation 的完整状态转换表**；
3. **commit 操作的端到端安全时序图、事实快照和崩溃恢复表**。

随后在 Windows 上进行 capability spike，验证：

- 独立身份或受限 token；
- Named Pipe/Local RPC 身份识别；
- 数据库、Blob、Git metadata 和凭据 ACL；
- Agent 只能写允许的源码路径；
- Agent 无法伪造 Human approval；
- Git commit 在各个崩溃窗口均可确定性 reconciliation。

只有这些安全和恢复前提得到验证后，再冻结技术栈、Schema 和首条写入型垂直切片，可以显著降低后续返工风险。
