# 产品定位

## 1. 产品定义

Agent Steward 是一个本地优先的个人 AI Agent 控制平面。它负责协调：

- 任务和批次；
- AI 会话、运行实例和上下文恢复；
- Registry Main、Task Owner 和子代理角色；
- Assignment、报告、事件和验收证据；
- Repository、Worktree 和 Git 生命周期；
- Herdr 与宿主原生 subagent；
- Prompt、流程和 Skill 的持续优化。

它不是一个新的 AI 模型，也不是单纯的待办列表或终端复用器。

## 2. 目标用户

V1 面向：

- 在本机使用一个或多个 AI Coding Agent 的个人开发者；
- 需要跨会话恢复复杂任务的用户；
- 同时使用 Herdr 和宿主原生 subagent 的用户；
- 希望逐步授权 AI 执行 commit、merge 或 push，同时保留可验证门禁的用户；
- 希望利用本地历史会话优化 Prompt、流程和 Skill 的用户。

## 3. 产品形态

```text
Human ── stewardctl / steward-ui ──┐
                                   ├── taskd
AI ──── steward-mcp / stewardctl ──┘
```

- `taskd`：唯一可信核心和运行时写入者。
- `stewardctl`：CLI，不因被人类调用就自动拥有管理员权限。
- `steward-mcp`：向 AI 暴露结构化工具，仍由服务端校验权限。
- Runtime Adapter：连接 Herdr、Pi、Claude、Codex 等宿主。

## 4. 差异化价值

与普通 Task Manager 相比：

- 会话、Assignment、Git 计划和证据是一等实体；
- 能区分任务完成、Agent 完成和 Git 集成完成；
- 能把用户批准绑定到具体计划和事实快照；
- 能基于长期本地历史发现重复流程和 Prompt 问题。

与普通 Agent Orchestrator 相比：

- 强调用户所有权和渐进授权；
- 强调本地数据、审计和可恢复性；
- 同时服务 Herdr 与原生 subagent；
- 不把 `completed` 事件直接当作业务验收完成。

## 5. 产品原则

1. **Local-first**：无云端依赖也能完整工作。
2. **User-owned**：用户决定数据、Git 和优化权限。
3. **Fail-closed**：身份、范围或前置事实不明确时拒绝写入。
4. **Single authority**：运行时只有一个权威数据库。
5. **Evidence over claims**：事实和证据优先于 Agent 自述。
6. **Progressive trust**：权限可从 deny/ask 逐步提升。
7. **Runtime neutral**：核心不依赖单一 Agent 宿主。
8. **Proposal before mutation**：高风险操作和自身优化先形成候选。
