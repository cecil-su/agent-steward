# 业务事实与 Git 变更流程图

本流程补充而不替代[任务管理流程图](18-task-management-workflow.md)。它描述工作台如何在 Git 分支、Worktree 和 Commit 不断变化时，持续保存业务事实，并把代码差异转化为可确认的映射、漂移或任务。

```mermaid
flowchart TB
    Start([打开 Workspace])
    LoadFacts[加载已确认业务事实、Wiki、场景图和任务]
    Discover[发现 Workspace 内的 Repository 与 Component]
    ReadGit[读取每个 Repo 的 Branch、Worktree、Commit 和 Diff]
    Snapshot[创建 Code Snapshot<br/>repoId + commitSha + observedAt]
    Scan[扫描 Component、API、数据模型、规则实现和测试]
    BuildMapping[建立业务事实到实现位置的候选映射]
    Compare[与上次快照和已确认业务事实比较]

    Drifted{是否发现差异或缺失}
    Refresh[更新 current 映射和 Repo Wiki 快照]
    CreateFinding[创建 Drift Finding<br/>保留事实，不自动覆盖]
    Classify{差异属于哪一类}

    OldBranch[旧分支或实验分支]
    MarkException[记录 Branch Exception<br/>事实保持不变]

    CodeIssue[实现缺陷或遗漏]
    CreateTask[创建关联业务事实的 Task]
    Decompose[Task Owner 拆分 Repo / Component Assignment]
    Execute[人或 AI 在指定 Worktree 执行]
    Submit[提交 Artifact、Diff 和测试证据]
    TaskReview[依据业务事实与验收标准 Review]
    Accepted{验收是否通过}
    Rework[返工并保持 Task 进行中]
    Integrate[合并或记录完成结果]

    FactChanged[需求或业务规则确实变化]
    FactCandidate[创建 Business Fact Revision Candidate]
    FactReview[用户审查来源、影响范围和业务场景图]
    FactAccepted{是否确认新事实}
    RejectFact[拒绝候选，保留原事实]
    Supersede[确认新版本并 supersede 旧事实]
    UpdateWiki[更新业务 Wiki、场景图和验收条件]
    Impact[生成受影响 Repo、Component 和 Task 清单]

    Rescan[重新扫描目标 Commit]
    Resolved{漂移是否解除}
    CloseFinding[关闭 Drift Finding 并更新时间线]
    KeepOpen[保持 finding，标记 stale / missing]

    Switch{Git 是否再次切换}
    Persist[业务事实、Wiki 和 Task 保持可用]
    End([继续工作台循环])

    Start --> LoadFacts
    LoadFacts --> Discover
    Discover --> ReadGit
    ReadGit --> Snapshot
    Snapshot --> Scan
    Scan --> BuildMapping
    BuildMapping --> Compare
    Compare --> Drifted

    Drifted -->|否| Refresh
    Refresh --> Switch

    Drifted -->|是| CreateFinding
    CreateFinding --> Classify

    Classify -->|旧分支或实验实现| OldBranch
    OldBranch --> MarkException
    MarkException --> Switch

    Classify -->|代码不符合事实| CodeIssue
    CodeIssue --> CreateTask
    CreateTask --> Decompose
    Decompose --> Execute
    Execute --> Submit
    Submit --> TaskReview
    TaskReview --> Accepted
    Accepted -->|否| Rework
    Rework --> Execute
    Accepted -->|是| Integrate
    Integrate --> Rescan

    Classify -->|业务事实已经变化| FactChanged
    FactChanged --> FactCandidate
    FactCandidate --> FactReview
    FactReview --> FactAccepted
    FactAccepted -->|否| RejectFact
    RejectFact --> Switch
    FactAccepted -->|是| Supersede
    Supersede --> UpdateWiki
    UpdateWiki --> Impact
    Impact --> CreateTask

    Rescan --> Resolved
    Resolved -->|是| CloseFinding
    Resolved -->|否| KeepOpen
    CloseFinding --> Switch
    KeepOpen --> Switch

    Switch -->|是| Persist
    Persist --> ReadGit
    Switch -->|否| End
```

## 分支切换不变量

无论 Git 如何切换，以下内容保持存在：

- Business System、Scenario、Requirement、Rule、Decision 和 Glossary；
- 已确认的业务 Wiki、场景图和事实版本历史；
- Task、Owner、Acceptance Criteria、Review 和 Artifact；
- 事实与历史 Code Snapshot 的映射和漂移记录。

切换后允许变化的是：

- 当前 Branch、Worktree、Commit 和 Diff；
- 当前代码中可观察到的 Component、API、表、Symbol 和测试结果；
- 实现映射的 `current / stale / missing` 状态；
- 基于新快照产生的 Wiki、图或事实候选。

## 关键规则

1. 先加载稳定业务事实，再观察当前 Git 现场。
2. 代码扫描结果必须绑定 `repoId + commitSha`，不能覆盖没有版本信息的知识。
3. 发现不一致时先创建 Drift Finding，由用户判断是代码问题、事实变化还是分支例外。
4. 如果代码有问题，创建以业务事实为验收依据的 Task，并把 Repo/Worktree 约束下放到 Assignment。
5. 如果业务事实变化，创建新 revision；确认后 supersede 旧版本，不删除历史。
6. Repo Wiki 和业务场景图的 AI 更新先进入 candidate，确认后才成为长期知识。
