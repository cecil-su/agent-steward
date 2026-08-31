# 任务管理流程图

本流程只描述任务从收集、整理、执行、阻塞、验收到完成的生命周期，不依赖 AI 才能成立。

```mermaid
flowchart TB
    Start([产生任务想法或工作请求])

    Capture[通过 TUI 或 GUI 快速录入]
    Inbox[进入 Inbox]
    Triage[Task Manager 整理任务]

    CompleteInfo{信息是否足够}
    Refine[补充目标、说明和项目归属]

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
    Review[进入 Review]
    Check[检查验收条件、产物和记录]
    Accepted{是否验收通过}

    Rework[记录问题并要求返工]
    Done[进入 Done]
    Archive[归档]
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

`Cancelled` 和 `Archived` 是终止或收纳状态。AI 执行者未来也必须遵循同一 Task 生命周期，不能建立另一套 AI 专用任务状态。
