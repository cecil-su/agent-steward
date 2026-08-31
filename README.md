# Agent Steward

Agent Steward 是一个本地优先、面向个人开发者的 AI Agent 控制平面。它统一管理任务、会话、角色授权、子代理、Git 生命周期、审计证据和提示词/Skill 优化，并通过 CLI 与 MCP 同时服务人类和 AI。

项目当前处于 **V1 设计阶段**，尚未开始实现。

## 核心原则

- 本地优先，默认不上传遥测或会话数据。
- SQLite 是运行时唯一权威；Markdown 仅作可读导出，不形成双重权威。
- CLI 和 MCP 都是无特权客户端，所有权限由可信核心统一判定。
- Agent 通过委派 grant 获得角色，不能自我声明或提升权限。
- Git 生命周期采用 Plan → Approve → Execute → Verify。
- 同时支持 Herdr 与宿主原生 subagent，并通过 Runtime Adapter 解耦。
- 完整保存本地会话数据，但保留来源、加密、导出和删除能力。
- 自优化遵循观察、提案、评估、授权、发布和回滚流程，不静默修改自身。

## V1 文档

从 [docs/v1/README.md](docs/v1/README.md) 开始阅读。

## 暂定组件名

| 组件 | 用途 |
|---|---|
| `taskd` | 可信核心服务、状态机、权限、审计和执行器 |
| `stewardctl` | 人类与受限 Agent 使用的 CLI |
| `steward-mcp` | MCP Server/Bridge |
| `steward-ui` | 用户审批与本地管理界面（可后续实现） |

组件名称仍属于 V1 设计项，在公开发布前需检查 GitHub、包管理器和可执行文件名冲突。
