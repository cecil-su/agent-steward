# 任务管理流程图

本流程描述 Phase 2 的任务生命周期，假设 Phase 1 已建立 Workspace/Repository/Worktree Registry。任务必须归属 Workspace，但不要求 Project；该流程不依赖 AI 才能成立。

```mermaid
flowchart TB
    Start([产生任务想法或工作请求])

    Capture[通过 TUI 或 GUI 快速录入]
    Inbox[进入 Inbox]
    Triage[Task Manager 整理任务]

    CompleteInfo{信息是否足够}
    Refine[补充目标、说明和 Workspace/Repo 上下文]

    Structure[设置优先级、依赖和父子关系]
    Acceptance[定义验收条件]
    Assign[指定 Task Owner]
    Next[确定唯一下一步]
    Ready[进入 Ready]

    StartWork[Task Owner 开始任务]
    InProgress[进入 In Progress]
    Work[执行当前 Next Action]

    Blocked{是否被阻塞}
    BlockState[进入 Blocked]
    RecordBlocker[记录阻塞原因和解除条件]
    Resolve[Task Manager 或 Owner 处理阻塞]
    Resolved{阻塞是否解除}

    Result{是否产生可验收结果}
    Continue[更新进度并确定下一步]
    Review[创建版本化 ReviewSubmission<br/>进入 Review]
    Check[按已绑定 Task/criteria/evidence 版本验收]
    Accepted{是否验收通过}

    Rework[原子写 changes_requested Decision<br/>退回 In Progress]
    Done[原子写 accepted Decision<br/>进入 Done]
    Archive[设置 archiveState = archived]
    SelectNext[Task Manager 选择下一任务]
    End([进入下一轮])

    Start --> Capture
    Capture --> Inbox
    Inbox --> Triage
    Triage --> CompleteInfo

    CompleteInfo -->|否| Refine
    Refine --> Triage
    CompleteInfo -->|是| Structure

    Structure --> Acceptance
    Acceptance --> Assign
    Assign --> Next
    Next --> Ready

    Ready --> StartWork
    StartWork --> InProgress
    InProgress --> Work
    Work --> Blocked

    Blocked -->|是| BlockState
    BlockState --> RecordBlocker
    RecordBlocker --> Resolve
    Resolve --> Resolved

    Resolved -->|否| BlockState
    Resolved -->|是| Next

    Blocked -->|否| Result
    Result -->|尚未完成| Continue
    Continue --> Next

    Result -->|完成候选| Review
    Review --> Check
    Check --> Accepted

    Accepted -->|否| Rework
    Rework --> InProgress

    Accepted -->|是| Done
    Done --> Archive
    Done --> SelectNext
    SelectNext --> End
```

## 默认状态

```text
Inbox → Ready → In Progress → Blocked → Review → Done
```

`Cancelled` 是业务终态。归档是与生命周期正交的收纳属性：它设置 `archiveState=archived` 和 archivedAt，但保留 Done、Cancelled 等原 status；恢复归档后仍从保留的 status 继续。AI 执行者未来也必须遵循同一 Task 生命周期，不能建立另一套 AI 专用任务状态。

每次进入 Review 都使用新的 reviewCycle；submit 为每个 source evidence 创建由 ReviewSubmission 独立持有的 active `review_evidence` Link，并固定 submittedTaskVersion、acceptanceCriteriaHash 和 evidence set。accept/request-changes 同时校验 Task 与 Submission 版本；accept 另外校验 submission-owned Link/version/contentHash，request-changes 可把证据缺失作为返工理由。两者都在同一事务写 Decision 和状态转换；Done 重新打开固定回到 In Progress，历史 accepted Decision 不得用于再次完成任务。
