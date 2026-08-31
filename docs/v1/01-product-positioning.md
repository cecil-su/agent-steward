# 产品定位

## 1. 产品定义

Agent Steward 是一个本地优先的个人任务管理器。它的产品价值按以下顺序建立：

1. 管好任务：收集、分解、排序、分配、推进、解除阻塞、验收和复盘；
2. 接入 AI：让 AI 与人类共享同一任务、owner、状态、产物和验收流程；
3. 改进系统：从任务历史中整理需求与偏好，并提出流程、Prompt 和 Skill 优化候选。

它不是一个以 Agent 编排为前提的控制台，也不是只记录标题和截止日期的待办清单。即使完全关闭 AI，它仍应是一套可独立使用的任务管理工具。

## 2. 核心用户与场景

V1 首先面向管理个人开发项目的用户：

- 快速把想法、问题和承诺收进 Inbox；
- 将任务归入项目，明确优先级、依赖、owner、验收标准和唯一下一步；
- 在 TUI 中快速操作，在 GUI 中浏览全局、关系和历史；
- 从 Blocked、Review 和逾期任务中找出真正需要处理的事项；
- 后续把适合的任务交给 AI，同时保留人工验收和完整证据；
- 长期积累项目需求和个人偏好，但避免未经确认的自动推断污染正式规则。

## 3. 产品形态

```text
Human ── TUI / GUI / CLI ── Command · Query · Event ── taskd ── SQLite
                                                     │
AI ───── MCP / Runtime Adapter ──────────────────────┤  第二阶段
                                                     │
Optimization Steward ◀──── Task Event History ───────┘  第三阶段
```

- `taskd`：任务状态、业务规则、查询、事件和持久化的唯一权威。
- `steward-tui`：键盘优先的捕获、筛选和推进界面。
- `steward-gui`：面向全局视图、依赖关系、时间线和审查的桌面界面。
- `stewardctl`：面向脚本和自动化的命令接口。
- `steward-mcp` / Runtime Adapter：第二阶段把 AI 接入现有任务系统。
- Optimization Steward：第三阶段在任务循环外生成待确认的改进候选。

## 4. 角色边界

- **Human Director**：决定目标、优先级和最终验收。
- **Task Manager**：只负责管理任务池，进行分流、排序、分配、跟踪和升级阻塞。
- **Task Owner**：对一个具体任务的推进、下一步和交付负责。
- **Worker**：执行具体工作，可以是人、AI 或自动化。
- **Context Steward**：循环外整理需求、偏好和优化候选，不直接改变任务结论。

Task Manager 不应同时承担“亲自完成所有任务”的职责；Task Owner 也不能凭执行者的完成声明跳过 Review。

## 5. 差异化价值

- **Actor-neutral**：同一 Task 可以由人、AI 或自动化拥有和执行。
- **双界面同状态**：TUI 与 GUI 不是两套产品，任何一端的变更都会通过事件同步。
- **事件化历史**：状态、owner、阻塞、验收和产物变化都有可恢复的时间线。
- **执行与验收分离**：Worker 完成意味着进入 Review，不等于任务已经 Done。
- **渐进式 AI**：AI 是增强层，不侵入任务管理核心，也不制造第二套状态真相。
- **候选式学习**：需求、偏好和 Prompt 优化先提供证据与差异，再由用户确认。

## 6. 产品原则

1. **Task first**：先把任务闭环做好，再增加 AI 自治。
2. **Local-first**：无云端或 AI 依赖也能完整工作。
3. **Single authority**：SQLite 是运行时唯一权威。
4. **One core, many clients**：TUI、GUI、CLI 和 MCP 共享业务规则。
5. **Explicit ownership**：每个活跃任务都有明确 owner 和下一步。
6. **Evidence before done**：验收依据优先于执行者自述。
7. **Progressive trust**：AI 和高风险操作逐步授权。
8. **Proposal before mutation**：需求、偏好和系统优化未经确认不生效。
