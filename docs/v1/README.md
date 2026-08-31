# Agent Steward V1 设计文档

状态：**设计基线草案**  
目标：在编码前明确产品边界、威胁模型、权威数据、角色授权、Git 门禁、Runtime 适配和自优化机制。

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

## 已确认方向

- 产品将作为独立的个人工具开发，并计划后续公开到 GitHub。
- 定位类似 Codegraph 的本地工具：CLI 提供人类入口，MCP 为 AI 提供结构化能力。
- 需要防范受到 Prompt Injection 影响、拥有普通项目 shell 权限的 Agent；不以抵御本机管理员或恶意软件为目标。
- Git inspect、commit、merge、push 均可纳入工具，但具体授权边界由用户按操作、任务和仓库决定。
- SQLite 作为运行时唯一权威数据源。
- 第一版数据全部保留在本地，默认无遥测上传；目标是理解用户意图、发现重复流程并产生 Prompt/Skill 优化候选。
- Herdr 和宿主原生 subagent 都必须支持。
- Optimizer 分阶段授权；默认只能观察和提出候选，用户授权后才允许进入实现和应用阶段。
- AI 可以使用 CLI 或 MCP；两者都必须受同一个可信服务和 capability 约束。

## 非目标

- V1 不提供云端多租户服务。
- V1 不以团队协作、组织级 RBAC 或跨机器集群为首要目标。
- V1 不允许 Agent 静默修改安全策略、自我提权或删除审计记录。
- V1 不把 Herdr、Pi、Claude、Codex 或任一 Git 平台写死在核心领域模型中。
