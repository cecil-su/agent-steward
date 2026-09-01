# TUI 与 GUI 交互架构图

TUI 和 GUI 是同一 taskd 的两个一等客户端。二者共享命令契约、查询模型、事件流和权限判定，不维护彼此独立的业务状态。本图包含后续 Task、AI 和优化入口；Phase 1 先交付 Workspace/Repo onboarding 与概览，任务核心边界见[任务管理器架构图](17-task-manager-architecture.md)。

```mermaid
flowchart TB
    User[用户]

    subgraph Clients[交互客户端]
        TUI[TUI<br/>快速命令、监控、终端审批]
        GUI[GUI<br/>Workspace/Repo 概览、任务看板、可视化审批]
        MCP[MCP<br/>AI 结构化调用]
    end

    subgraph Interface[统一交互层]
        Command[Command API<br/>创建、分配、推进、确认]
        Query[Query API<br/>Workspace/Repo、任务、需求、执行记录]
        Events[Event Stream<br/>进度、阻塞、产物、提案]
        Approval[Approval Service<br/>可信用户确认]
    end

    subgraph Delivery[工作上下文与任务交付]
        Registry[Workspace / Repo / Worktree Registry]
        Manager[Task Manager<br/>只管理任务池、状态和下一步]
        Owner[Task Owner<br/>管理单个任务和子代理]
        Subagents[Subagents<br/>Scout / Writer / Reviewer / Tester]
    end

    subgraph Learning[外层学习与优化循环]
        Steward[Context & Optimization Steward]
        Requirements[Workspace 业务事实与决策]
        Preferences[用户偏好<br/>确认 / 候选 / 任务级]
        Proposals[Prompt / Workflow / Skill 提案]
        Context[Context Brief Builder]
    end

    subgraph Runtime[执行与持久化]
        Taskd[taskd<br/>Application Service 与状态权威]
        DB[(SQLite)]
        Blob[(Artifact / Session Store)]
        Adapters[Runtime Adapters]
        Agents[Codex / Claude / Pi / Herdr]
        Git[Git 与验证工具]
    end

    User --> TUI
    User --> GUI

    TUI --> Command
    GUI --> Command
    MCP --> Command

    TUI --> Query
    GUI --> Query
    MCP --> Query

    Command --> Taskd
    Query --> Taskd
    Approval --> Taskd
    Taskd --> Events
    Events --> TUI
    Events --> GUI
    Events --> MCP

    Taskd --> Registry
    Registry -->|TaskContextBinding| Manager
    Manager -->|指定 Owner| Owner
    Owner -->|拆分 Assignment| Subagents
    Subagents --> Adapters
    Adapters --> Agents
    Agents -->|进度、结果与证据| Taskd
    Taskd --> Git

    Requirements --> Context
    Preferences --> Context
    Context --> Owner

    Events --> Steward
    Steward --> Proposals
    Proposals --> Approval
    User -->|确认或拒绝| Approval
    Approval -->|确认后更新| Requirements
    Approval -->|确认后更新| Preferences

    Taskd <--> DB
    Taskd <--> Blob
```

## 交互原则

- TUI 和 GUI 只负责呈现与发出命令，`taskd` 是唯一业务状态权威。
- 所有关键能力在两个客户端中语义一致；GUI 可以提供更丰富的看板、对比和审批体验。
- 在任一客户端完成操作后，另一客户端通过 Event Stream 实时更新。
- 客户端断线重连时，先通过 Query API 重建快照，再从事件游标继续消费。
- Context & Optimization Steward 位于任务交付循环之外，只能生成候选；长期需求、偏好和 Prompt 变更必须由用户确认。
