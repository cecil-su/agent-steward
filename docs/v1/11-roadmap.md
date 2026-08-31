# V1 实施路线

V1 采用可验证的垂直切片推进，不一次性实现所有自治能力。

## Phase 0：设计冻结与实验

目标：关闭会影响信任边界和存储的关键决策。

- 明确威胁模型和 Standard/Hardened 模式；
- 选择技术栈、SQLite/加密方案和 Local RPC；
- 选择可信用户审批方式；
- 选择首个 Native Subagent Host；
- 编写领域 schema、状态机和 Runtime conformance contract；
- 对 Windows sandbox/独立身份、Git metadata ACL 和凭据隔离做 capability probe。

退出条件：安全模型不存在“同一 Agent 可直接绕过”的未说明路径。

## Phase 1：只读控制平面

- taskd skeleton；
- SQLite schema/migration；
- 导入现有 Markdown 的一次性只读 importer；
- task/session/assignment 查询；
- stewardctl list/show/audit；
- MCP `task_list/get/confirmations/audit`；
- Git、Herdr、Session 只读事实采集；
- 默认无遥测验证。

退出条件：CLI/MCP 对同一事实返回一致结果，且无任何写 Git 生命周期。

## Phase 2：角色、事件和受控任务写入

- Principal、RoleGrant、Capability、Lease、owner epoch；
- Registry Main/Task Owner/child assignment；
- expectedVersion 和幂等；
- event outbox、至少一次投递、ack；
- append-only audit；
- Herdr Adapter；
- Manual Adapter。

退出条件：过期会话、越权角色和重复 event 均被确定性处理。

## Phase 3：双 Runtime 支持

- Runtime SDK 与 capability discovery；
- Herdr 完整生命周期；
- 一个具体 Native Host Plugin；
- spawn/resume/complete/blocked/reconciliation；
- Runtime conformance test suite。

退出条件：同一 Assignment contract 可在 Herdr 和 Native Runtime 下完成。

## Phase 4：Git Plan 与本地生命周期

- Git inspect；
- managed Worktree；
- Writer lease；
- stage/commit/merge plan；
- trusted approval；
- execute/verify/reconciliation；
- Hook 漂移和冲突测试。

退出条件：任何事实漂移都会使计划失效，Agent 不能通过 CLI/MCP 绕过 taskd。

## Phase 5：Push 与 Hardened 模式

- credential broker；
- push plan/approval/verify；
- Agent credential isolation；
- 独立 OS 身份或 sandbox；
- `.git` metadata 保护；
- 安全安装和降级说明。

退出条件：受限 Agent 不能直接使用用户凭据或原生 Git 绕过 push 策略。

## Phase 6：完整 Session Store 与 Optimizer L0–L2

- 多宿主 Session importer；
- encrypted blob；
- provenance；
- 本地索引与用户偏好候选；
- deterministic metrics；
- Optimizer observe/analyze/propose；
- Skill candidate 输出。

退出条件：原始数据不出本机，外部内容不会被当成用户偏好，候选不会自动应用。

## Phase 7：Optimizer L3–L4

- 隔离 candidate；
- replay/shadow/canary；
- 独立 Reviewer；
- 用户批准、版本发布和回滚；
- Prompt/Skill/Policy Pack registry。

退出条件：任何 live 优化都能追溯 proposal、证据、批准和回滚点。

## 发布建议

- `0.1.x`：只读 task/session/MCP。
- `0.2.x`：角色、assignment、event、Herdr/native。
- `0.3.x`：Git plan 和本地 commit/merge。
- `0.4.x`：Hardened push。
- `0.5.x`：Session analytics 和 Optimizer。
- `1.0.0`：安全边界、迁移和 adapter contract 稳定。
