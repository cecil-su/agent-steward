# V1 需求说明

## 1. 功能需求

### FR-001 任务管理

- 支持任务、批次父子关系、阶段、状态、阻塞条件和唯一下一步。
- 支持按完整任务标识、外部编号、状态和 owner 查询。
- 状态转换必须经过状态机和版本前置检查。

### FR-002 会话管理

- 保存 Session、Invocation、Assignment 和 AgentRun 的独立身份。
- 支持同一 Session 跨 Invocation 恢复。
- 恢复时必须重新校验任务事实、owner epoch 和授权范围。

### FR-003 角色与委派

- 支持 Human/Admin、Registry Main、Task Owner、Scout、Writer、Reviewer、Tester、Optimizer 等角色。
- 角色由上级 principal 派发 grant，Agent 不得自行声明或提权。
- Grant 可绑定 Task、Assignment、Worktree、路径、操作、期限和 owner epoch。

### FR-004 子代理编排

- 支持创建、恢复、查询和关闭 Agent Run。
- 同时支持 Herdr 和宿主原生 subagent。
- Runtime 状态只作为协调事实，不直接改变任务验收状态。

### FR-005 事件与 Artifact

- 支持至少一次事件投递、event ID 幂等和显式 ack。
- 支持报告、测试结果、Git 快照和上下文 checkpoint。
- event/report 不能自动等同于验收结论。

### FR-006 Git 治理

- 支持 Git 只读事实采集和受控生命周期操作。
- commit、merge、push 使用 Plan → Approve → Execute → Verify。
- 用户可按操作配置 deny、ask、allow-once、allow-task、allow-repo、allow-global。

### FR-007 本地数据

- SQLite 是结构化状态权威。
- 完整会话和大型 Artifact 保存在本地加密 Blob Store。
- 默认不上传遥测。
- 支持数据查看、导出、删除和保留策略。

### FR-008 CLI 与 MCP

- CLI 和 MCP 调用同一 Application Service。
- 两者均不携带固有管理员权限。
- MCP 返回结构化、边界明确且可机器校验的结果。

### FR-009 自身优化

- 统计重复流程、澄清、返工、finding、工具失败和上下文消耗。
- 区分 user、assistant、tool、repository、web 等内容来源。
- 生成 Prompt、流程和 Skill 候选。
- 未经用户授权不得修改 live 配置、规则或代码。

### FR-010 审计与恢复

- 所有写操作记录 principal、grant、输入版本、计划、结果和时间。
- 崩溃后能够判断操作未开始、执行中、已完成或需要人工协调。
- 审计日志不可由 Agent 删除或改写。

## 2. 非功能需求

### NFR-001 安全

- 防范受到 Prompt Injection 影响、拥有普通项目 shell 权限的 Agent 越权调用管理能力。
- 不以抵御本机管理员、恶意软件或操作系统攻破为目标。

### NFR-002 可移植性

- 核心不写死 Windows、Pi、Herdr、NRS 或单一 Git 平台。
- V1 优先保证 Windows 可用，同时设计跨平台路径和进程接口。

### NFR-003 可测试性

- 状态机、权限、幂等、Git 计划和 Runtime Adapter 必须可通过确定性测试验证。
- MCP 与 CLI 应共享相同契约测试。

### NFR-004 可观测性

- 关键操作提供结构化日志、关联 ID 和审计事件。
- 原始数据、衍生索引和优化结果应区分版本与来源。

### NFR-005 兼容性

- Runtime Adapter 必须声明能力，不假设所有宿主都支持 resume、pane、push notification 或 layout。

## 3. V1 验收原则

- 未授权 Agent 无法创建或接管 Task Owner grant。
- 过期 owner epoch 无法写入任务或执行 Git 操作。
- 同一操作通过 CLI 和 MCP 得到相同权限判定。
- Git 计划事实变化后不能继续执行。
- 会话数据在关闭网络的情况下可完整记录、查询和导出。
- Optimizer 只能生成候选，除非存在明确的更高等级授权。
