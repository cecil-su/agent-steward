# 任务接续核心架构

阶段 A–C 的最小核心以 Task 和 TaskCheckpoint 为中心。Task Manager 是用户管理任务时承担的角色，不要求独立的自动调度器。

```mermaid
flowchart TB
  Entry[CLI / 一个薄界面 / 现有 AI 会话] --> App[Application Service]
  App --> Task[Task Service<br/>目标、owner、下一步、状态]
  App --> Resume[Checkpoint / 恢复查询]
  App --> Review[Review / 用户验收]
  App --> Binding[Workspace / Repo / Worktree 关联]
  App --> Evidence[决策来源与证据]
  Task --> Rules[生命周期、版本、权限、幂等]
  Resume --> Task
  Resume --> Evidence
  Resume --> Binding
  Review --> Task
  Review --> Evidence
  Binding -.重新观察.-> Git[Git / 文件系统]
  App --> DB[(SQLite：领域状态与事件)]
  App --> Blob[启用时的 Artifact Store]
  Optional[子任务 / 依赖 / SavedView] -.需求验证后.-> App
  Runtime[可选 D：Assignment / AgentRun] -.同一任务.-> App
```

- Checkpoint 引用采集时的 Task 版本，不独立维护 lifecycle 或 owner。
- 恢复上下文展示当前事实与历史声明的差异，不能把摘要当作当前现场。
- Review 固定证据，由用户验收；owner 可以是人或 AI，但执行声明不能代替验收。
- 首期不要求 Next Action Engine、复杂依赖图或第二客户端；下一步可由当前 owner 明确维护。
