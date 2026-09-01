# Agent Steward V1 设计文档

状态：**任务管理优先的设计基线草案**

目标：先冻结一个不依赖 AI 也成立的任务管理器，再分阶段接入 AI 执行以及需求、偏好和流程优化能力。

## 建议阅读顺序

先读[产品定位](01-product-positioning.md)、[任务管理器架构图](17-task-manager-architecture.md)和[任务管理流程图](18-task-management-workflow.md)，再读[业务事实工作台架构图](19-business-fact-workbench-architecture.md)和[业务事实与 Git 变更流程图](20-business-fact-and-git-flow.md)，最后读[需求说明](02-requirements.md)、[总体架构](03-architecture.md)、[领域与数据模型](05-domain-model.md)和[V1 实施路线](11-roadmap.md)。第 13–16 篇描述 AI 与优化能力全部启用后的完整产品视图。

## 文档目录

1. [产品定位](01-product-positioning.md)
2. [需求说明](02-requirements.md)
3. [总体架构](03-architecture.md)
4. [安全与权限模型](04-security-model.md)
5. [领域与数据模型](05-domain-model.md)
6. [CLI 与 MCP 设计](06-cli-and-mcp.md)
7. [Git 生命周期治理](07-git-governance.md)
8. [Herdr 与原生 Subagent](08-runtime-adapters.md)
9. [会话数据、隐私与本地存储](09-session-data-and-privacy.md)
10. [自身优化与 Skill 生成](10-self-optimization.md)
11. [V1 实施路线](11-roadmap.md)
12. [待决策事项](12-open-decisions.md)
13. [产品架构图](13-product-architecture.md)
14. [核心业务流程图](14-core-workflow.md)
15. [TUI 与 GUI 交互架构图](15-tui-gui-interaction-architecture.md)
16. [TUI 与 GUI 协同交互流程图](16-tui-gui-interaction-flow.md)
17. [任务管理器架构图](17-task-manager-architecture.md)
18. [任务管理流程图](18-task-management-workflow.md)
19. [业务事实工作台架构图](19-business-fact-workbench-architecture.md)
20. [业务事实与 Git 变更流程图](20-business-fact-and-git-flow.md)

## 已确认方向

- 产品将作为独立的个人工具开发，并计划后续公开到 GitHub。
- 第一需求是任务管理；首个可用版本不能依赖 AI、MCP 或 Git 自动化才能成立。
- Task 是共享工作单元；项目、子任务、状态、依赖、owner、下一步、验收和历史是核心能力。
- TUI 与 GUI 都是一等客户端，调用同一套 Command、Query 和 Event API，并读取同一权威状态。
- 人类、AI 和自动化都使用通用 Actor/Owner 模型；Task Manager 只管理任务池，Task Owner 推进具体任务。
- AI 执行和流程体验属于第二优先级，必须接入同一任务生命周期，不能形成独立的“AI 任务系统”。
- 需求、用户偏好、流程与 Prompt 优化属于第三优先级，在任务循环外读取历史并生成候选。
- SQLite 作为运行时唯一权威数据源。
- 数据默认保留在本地且不上传遥测；所有任务变化记录为可追溯事件。
- Workspace 承载跨 Repo、跨 Branch 的业务事实；Git 只提供绑定 Commit 的实现快照和证据。
- Optimizer 默认只能观察和提出候选，用户确认后才允许应用。
- AI、高风险 Git 操作和多 Runtime 适配继续保留严格授权设计，但不阻塞任务管理 MVP。

## 非目标

- 首个里程碑不提供云端多租户、团队协作、组织级 RBAC 或跨机器集群。
- 首个里程碑不要求 AI 执行、多 Runtime、Git push 或完整会话采集。
- AI 能力不能成为创建、查看、推进和验收普通任务的前置条件。
- 优化模块不得静默修改用户偏好、项目需求、安全策略、Prompt 或代码。
- 核心领域模型不写死 Herdr、Pi、Claude、Codex 或任一 Git 平台。
