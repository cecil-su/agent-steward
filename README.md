# Agent Steward

Agent Steward 是一个本地优先、面向个人开发者的任务管理工具。它首先帮助用户可靠地收集、整理和推进任务；随后让人类与 AI 共享同一套任务、状态、产物和验收流程；最后基于长期任务历史整理项目需求、用户偏好，并提出流程与 Prompt 优化候选。

项目当前处于 **V1 设计阶段**，尚未开始实现。

## 产品优先级

1. **任务管理**：先成为一个不依赖 AI 也好用的任务管理器。
2. **AI 执行与流程体验**：AI 作为 Actor 接入同一任务模型，不另建一套任务系统。
3. **需求、偏好与优化**：在任务循环外观察历史，生成候选并由用户确认。

## 核心原则

- Task 是第一等领域对象，AI、Git 和优化能力都是可选扩展。
- 本地优先；SQLite 是运行时唯一权威，Markdown 仅作可读导出。
- TUI 和 GUI 调用相同的 Command、Query 与 Event API，不维护独立状态。
- 人类、AI 和自动化统一建模为 Actor；Task Manager 管理任务池，Task Owner 推进具体任务。
- AI 声明完成只会进入 Review，验收通过后任务才能进入 Done。
- 所有状态变化写入 Task Event，以便追踪、恢复和审计。
- 需求、偏好、流程和 Prompt 优化只生成候选，未经用户确认不进入正式配置。

## V1 文档

从 [docs/v1/README.md](docs/v1/README.md) 开始阅读。

## 暂定组件名

| 组件 | 用途 |
|---|---|
| `taskd` | 本地任务核心、状态机、查询、事件和持久化 |
| `steward-tui` | 键盘优先的终端任务界面 |
| `steward-gui` | 可视化任务管理界面 |
| `stewardctl` | 脚本和自动化使用的 CLI |
| `steward-mcp` | 第二阶段供 AI 使用的 MCP Bridge |
| Runtime Adapter | 第二阶段连接不同 AI/Agent 宿主 |
| Optimization Steward | 第三阶段整理需求、偏好与优化候选 |

组件名称仍属于 V1 设计项，在公开发布前需检查 GitHub、包管理器和可执行文件名冲突。
