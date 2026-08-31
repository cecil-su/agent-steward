# 任务管理器架构图

本图只描述第一优先级的任务管理器。TUI 与 GUI 是同一任务核心的两个客户端；AI、需求整理和流程优化仅作为后续扩展点。

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
        Events[Event Stream<br/>状态变化、阻塞、评论、提醒]
    end

    subgraph Core[任务管理核心]
        Project[Project Service]
        Task[Task Service]
        Lifecycle[Lifecycle Engine<br/>状态转换与规则]
        Relation[Dependency Service<br/>父子、依赖、阻塞]
        Ownership[Ownership Service<br/>Task Manager / Task Owner]
        NextAction[Next Action Engine]
        Review[Review & Acceptance]
        Timeline[Task Timeline]
    end

    subgraph Domain[核心数据模型]
        ProjectEntity[Project]
        TaskEntity[Task / Subtask]
        ActorEntity[Actor / Owner]
        RelationEntity[Task Relation]
        EventEntity[Task Event]
        ArtifactEntity[Comment / Attachment / Artifact]
    end

    subgraph Storage[本地数据层]
        DB[(SQLite<br/>唯一状态权威)]
        Files[(本地附件与产物)]
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

    Project --> ProjectEntity
    Task --> TaskEntity
    Ownership --> ActorEntity
    Relation --> RelationEntity
    Timeline --> EventEntity
    Review --> ArtifactEntity

    Task --> Lifecycle
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
- 任一客户端修改后，另一客户端通过 Event Stream 同步。
- Task 是第一等核心实体；AI 后续通过通用 Actor、Execution 和 Adapter 接入。
- 需求、偏好和流程优化读取任务历史，但不进入第一阶段的任务写入主链路。
