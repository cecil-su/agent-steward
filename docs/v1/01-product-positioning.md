# 产品定位

## 1. 产品定义

Agent Steward 是一个本地优先的个人开发 Workspace 工作管家。它的产品价值按以下顺序建立：

1. 建立工作边界：注册 Workspace，可靠识别其中的 Repository、Worktree、Branch、HEAD 和只读工作区状态；
2. 管好任务：让 Task 归属 Workspace，并按需关联 Repo/Worktree，完成收集、推进、验收和复盘；
3. 接入 AI：让 AI 与人类共享同一任务、owner、状态、产物和验收流程；
4. 建立长期认知：沉淀业务事实、实现快照和映射，再提出流程、Prompt 和 Skill 优化候选。

它不是一个以 Agent 编排为前提的控制台，也不是另一个要求用户先建立抽象 Project 层级的项目管理器。即使完全关闭 AI，首期也能作为可靠的本地 Workspace/Repo Registry 使用；任务能力上线后仍不依赖 AI 才能成立。

## 2. 核心用户与场景

V1 首先面向同时维护一个或多个本地 Repository/Worktree 的个人开发者：

- 创建或打开 Workspace，自动发现并确认其中的 Repository/Worktree；
- 查看 canonical path、remote identity、Branch、HEAD、dirty/missing 状态，而不修改 Git 现场；
- 在第二阶段把想法、问题和承诺收进 Workspace Inbox，并将 Task 关联相关 Repo/Worktree；
- 明确优先级、依赖、owner、验收标准和唯一下一步；
- 在 TUI 中快速操作，在 GUI 中浏览全局、关系和历史；
- 从 Blocked、Review 和逾期任务中找出真正需要处理的事项；
- 后续把适合的任务交给 AI，同时保留人工验收和完整证据；
- 长期积累 Workspace 业务事实和个人偏好，但避免未经确认的自动推断污染正式规则。

## 3. 产品形态

```text
Human ── TUI / GUI / CLI ── Command · Query · Event ── taskd ── SQLite
                                                     │
Task / Review ───────────────────────────────────────┤  第二阶段
AI ───── MCP / Runtime Adapter ──────────────────────┤  第三阶段
                                                     │
Knowledge / Optimization ◀── Domain Event History ───┘  后续阶段
```

- `taskd`：Workspace/Repo Registry、任务状态、业务规则、查询、事件和持久化的唯一权威。
- `steward-tui`：键盘优先的 Workspace/Repo 查看，以及后续任务捕获、筛选和推进界面。
- `steward-gui`：面向 Workspace/Repo 概览，以及后续依赖关系、时间线和审查的桌面界面。
- `stewardctl`：面向脚本和自动化的命令接口。
- `steward-mcp` / Runtime Adapter：第三阶段把 AI 接入现有任务系统。
- Optimization Steward：后续阶段在任务循环外生成待确认的改进候选。

## 4. 角色边界

- **Human Director**：决定目标、优先级和最终验收。
- **Task Manager**：只负责管理任务池，进行分流、排序、分配、跟踪和升级阻塞。
- **Task Owner**：对一个具体任务的推进、下一步和交付负责。
- **Worker**：执行具体工作，可以是人、AI 或自动化。
- **Context Steward**：循环外整理需求、偏好和优化候选，不直接改变任务结论。

Task Manager 不应同时承担“亲自完成所有任务”的职责；Task Owner 也不能凭执行者的完成声明跳过 Review。

## 5. 差异化价值

- **Actor-neutral**：同一 Task 可以由人、AI 或自动化拥有和执行。
- **Context before workflow**：先确定 Workspace/Repo/Worktree 身份，再把任务、证据和执行绑定到真实工作上下文。
- **双界面同状态**：TUI 与 GUI 不是两套产品，任何一端的变更都会通过事件同步。
- **事件化历史**：状态、owner、阻塞、验收和产物变化都有可恢复的时间线。
- **执行与验收分离**：Worker 完成意味着进入 Review，不等于任务已经 Done。
- **渐进式 AI**：AI 是增强层，不侵入任务管理核心，也不制造第二套状态真相。
- **候选式学习**：需求、偏好和 Prompt 优化先提供证据与差异，再由用户确认。

## 6. 产品原则

1. **Workspace first**：先建立 Workspace、Repository 和 Worktree 的稳定本地身份，再增加 Task 与 AI。
2. **Local-first**：无云端或 AI 依赖也能完整工作。
3. **Single authority**：SQLite 是运行时唯一权威。
4. **One core, many clients**：TUI、GUI、CLI 和 MCP 共享业务规则。
5. **Read-only discovery first**：注册、扫描和解除关联不修改 Git，也不删除用户目录。
6. **Explicit ownership**：每个活跃任务都有明确 owner 和下一步。
7. **Evidence before done**：验收依据优先于执行者自述。
8. **Progressive trust**：AI 和高风险操作逐步授权。
9. **Proposal before mutation**：需求、偏好和系统优化未经确认不生效。
