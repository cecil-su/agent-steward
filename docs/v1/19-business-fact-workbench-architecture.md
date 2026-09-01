# 业务事实工作台架构图

本图补充而不替代[任务管理器架构图](17-task-manager-architecture.md)。任务管理器负责推进工作；业务事实层负责保存不会随 Branch、Worktree 或 Commit 切换而消失的项目逻辑。Repository 是实现载体和证据来源，不是业务事实的唯一权威。

```mermaid
flowchart TB
    User[用户]

    subgraph Clients[交互入口]
        TUI[TUI<br/>任务、事实查询、快速确认]
        GUI[GUI<br/>业务 Wiki、场景图、漂移审查]
    end

    subgraph API[统一应用接口]
        Command[Command API]
        Query[Query API]
        Events[Event Stream]
    end

    subgraph Workbench[Workspace Core]
        Workspace[Workspace Registry]
        Facts[Business Fact Service]
        Tasks[Task Service]
        Knowledge[Wiki & Diagram Service]
        Mapping[Implementation Mapping]
        Drift[Fact Drift Detection]
        Review[Confirmation & Review]
    end

    subgraph Business[稳定业务事实层]
        System[Business System]
        Scenario[Business Scenario]
        Requirement[Requirement]
        Rule[Business Rule]
        Decision[Decision]
        Glossary[Glossary / Domain Entity]
        FactRevision[Fact Revision<br/>candidate / confirmed / superseded]
    end

    subgraph Execution[任务执行层]
        Task[Task]
        Assignment[Assignment]
        Acceptance[Acceptance Criteria]
        Artifact[Review Evidence / Artifact]
    end

    subgraph Implementation[实现认知层]
        Repo[Repository]
        Component[Component / App / Service]
        ImplRef[Implementation Reference<br/>API、表、文件、Symbol]
        CodeSnapshot[Code Snapshot<br/>repoId + commitSha]
        MappingState[Mapping State<br/>current / stale / missing]
    end

    subgraph GitRuntime[Git 运行现场]
        Branch[Branch]
        Worktree[Worktree]
        Commit[Commit]
        Diff[Diff / Test Result]
    end

    subgraph Storage[本地持久化]
        DB[(Workspace SQLite<br/>业务事实、任务与关系权威)]
        Files[(Knowledge Artifact Store<br/>Wiki、Mermaid、附件)]
        Index[(Search / Knowledge Index)]
    end

    subgraph OptionalAI[可选 AI 增强]
        Extractor[Fact & Code Extractor]
        Context[Context Brief Builder]
        Runtime[Agent Runtime Adapter]
    end

    User --> TUI
    User --> GUI
    TUI --> Command
    TUI --> Query
    GUI --> Command
    GUI --> Query
    Events --> TUI
    Events --> GUI

    Command --> Workbench
    Query --> Workbench
    Workbench --> Events

    Facts --> Business
    Tasks --> Execution
    Knowledge --> System
    Knowledge --> Scenario
    Knowledge --> Files
    Mapping --> Implementation
    Drift --> MappingState
    Review --> FactRevision
    Review --> Acceptance

    System --> Scenario
    Scenario --> Requirement
    Scenario --> Rule
    Requirement --> Acceptance
    Decision --> FactRevision
    Glossary --> FactRevision

    Task --> Scenario
    Task --> Requirement
    Task --> Acceptance
    Task --> Assignment
    Assignment --> Repo
    Assignment --> Component
    Assignment --> Worktree
    Artifact --> Review

    Repo --> Component
    Component --> ImplRef
    ImplRef --> MappingState
    CodeSnapshot --> ImplRef
    Branch --> Commit
    Worktree --> Commit
    Commit --> CodeSnapshot
    Diff --> CodeSnapshot

    Facts <--> DB
    Tasks <--> DB
    Mapping <--> DB
    Knowledge <--> DB
    Query <--> Index

    CodeSnapshot -.只提供实现证据.-> Mapping
    Drift -.生成候选，不直接改事实.-> Review
    Extractor -.生成候选事实与映射.-> Review
    Context -.组合已确认事实与当前代码快照.-> Runtime
    Runtime -.通过 Assignment 执行.-> Tasks
```

## 权威边界

- Workspace 是长期知识和任务的边界，可以包含多个 Repository。
- SQLite 保存结构化业务事实、任务、确认状态、关系和版本；Wiki 与图保存在本地 Artifact Store，并由稳定 ID 引用。
- Business System、Scenario、Requirement、Rule 和 Decision 不使用 Branch、路径或 Commit SHA 作为身份。
- Repository、Component、API、表和 Symbol 属于实现认知层，必须通过带 `commitSha` 的 Code Snapshot 说明观察版本。
- Git 切换只产生新的 Code Snapshot，并更新实现映射状态；不会删除或静默改写已确认业务事实。
- Repo Wiki 中的稳定说明属于 Workspace Knowledge；自动扫描得到的目录、API 和依赖信息属于带版本的实现快照。
- AI 只能生成 candidate、mapping 或 drift finding，正式业务事实需要用户或授权 Reviewer 确认。

## 三个核心主对象

```text
Business Fact        项目为什么这样运作，跨 Git 版本长期存在
Task                 当前需要推进什么，以业务事实作为目标和验收依据
Repository Snapshot  当前代码怎样实现这些事实，随 Commit 变化
```
