# 任务管理器架构图

本图描述 Phase 2 的任务与 Review 闭环，并假设 Phase 1 已提供 Workspace、Repository 和 Worktree Registry。TUI 与 GUI 是同一任务核心的两个客户端；AI、业务事实和流程优化仅作为后续扩展点。

```mermaid
flowchart TB
    User[用户]

    subgraph Clients[交互层]
        TUI[TUI<br/>快速录入、命令、列表、实时状态]
        GUI[GUI<br/>看板、依赖、详情、时间线]
    end

    subgraph Interface[统一接口层]
        Commands[Command API<br/>创建、修改、推进、归档]
        Queries[Query API<br/>搜索、过滤、详情、视图]
        Events[Event Stream<br/>全局 streamPosition、状态变化、评论]
    end

    subgraph Core[任务管理核心]
        Context[Workspace / Repo Context Service]
        Task[Task Service]
        Lifecycle[Lifecycle Engine<br/>状态转换与规则]
        Relation[Dependency Service<br/>父子、依赖、阻塞]
        Ownership[Ownership Service<br/>Task Manager / Task Owner]
        NextAction[Next Action Engine]
        Review[Review & Acceptance]
        Timeline[Task Timeline]
    end

    subgraph Domain[核心数据模型]
        ContextEntity[Workspace / Repository / Worktree<br/>TaskContextBinding]
        TaskEntity[Task / Subtask]
        ActorEntity[Actor / Owner]
        RelationEntity[Task Relation]
        EventEntity[Task Event]
        EvidenceEntity[Comment / ArtifactLink<br/>ReviewSubmission / ReviewDecision]
    end

    subgraph Storage[本地数据层]
        DB[(SQLite<br/>唯一状态权威)]
        Files[(通用 Artifact Store)]
        Index[(搜索索引)]
    end

    subgraph Extensions[后续扩展点]
        AI[AI Execution Adapter]
        Knowledge[需求与偏好整理]
        Integration[Git / Calendar / 外部工具]
    end

    User --> TUI
    User --> GUI

    TUI --> Commands
    GUI --> Commands
    TUI --> Queries
    GUI --> Queries

    Commands --> Core
    Queries --> Core
    Core --> Events
    Events --> TUI
    Events --> GUI

    Context --> ContextEntity
    Task --> TaskEntity
    Ownership --> ActorEntity
    Relation --> RelationEntity
    Timeline --> EventEntity
    Review --> EvidenceEntity

    Task --> Lifecycle
    Task --> Context
    Task --> Relation
    Task --> Ownership
    Task --> NextAction
    Task --> Review
    Lifecycle --> Timeline

    Core <--> DB
    Core <--> Files
    Queries <--> Index

    AI -.通过 Actor / Execution 接入.-> Core
    Knowledge -.读取任务历史.-> Events
    Integration -.通过适配器接入.-> Core
```

## 架构原则

- TUI 和 GUI 不维护独立业务状态。
- 所有写入使用同一 Command API，所有查询来自同一 SQLite 权威数据源。
- 任一客户端修改后，另一客户端从同事务 snapshot + eventWatermark 继续通过全局 streamPosition 同步。
- Task 是 Phase 2 的核心工作实体，必须归属 Workspace，并可通过 TaskContextBinding 关联 Repo/Worktree；AI 后续通过通用 Actor、Execution 和 Adapter 接入。
- 业务事实、偏好和流程优化读取任务历史，但不进入 Phase 2 的任务写入主链路。
