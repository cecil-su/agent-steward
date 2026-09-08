# 任务生命周期与恢复

生命周期服务于任务接续与用户验收。Workspace 可以隐式建立，Repository 关联可选；任务推进不依赖 AI、Assignment 或 Runtime。

```mermaid
stateDiagram-v2
  state "In Progress" as InProgress
  [*] --> Inbox: 创建任务
  Inbox --> Ready: 明确目标与下一步
  Ready --> InProgress: 指定 owner 并开始
  InProgress --> Blocked: 记录阻塞原因
  Blocked --> InProgress: 恢复执行
  InProgress --> Review: 固定版本与证据并提交
  Review --> InProgress: 用户要求返工
  Review --> Done: 用户验收通过
  Done --> InProgress: 用户重新打开
  Inbox --> Cancelled: 用户取消
  Ready --> Cancelled: 用户取消
  InProgress --> Cancelled: 用户取消
  Blocked --> Cancelled: 用户取消
  Review --> InProgress: 有权限的 owner 或用户撤回，再编辑或取消
```

Review 显式撤回与 Blocked 回到 In Progress 已按第 21 篇冻结；其余生命周期前置条件由 D-013 冻结。Review 中的业务编辑、取消和归档均需先撤回，Comment/Checkpoint 独立追加不增加 Task version。

## 接续不改变生命周期

- 任一允许记录的非终态任务可以保存 TaskCheckpoint；保存后保持原状态。
- 会话结束、模型窗口丢失或工具重启不产生 Done/Cancelled。
- 恢复先读取当前 Task、owner、证据与 Git，再参考最近 Checkpoint；缺失引用显式标注。
- 恢复旧 Checkpoint 不等于恢复旧权限，也不意味着回滚 Task 版本。

## Review 与归档

submit 为每个证据创建 Submission 独立持有的 review_evidence Link，固定 Task/criteria/evidence 版本；用户 accept/request-changes 原子写 Decision 和状态转换。Review 期间撤回与重提遵循第 21 篇/D-020；撤回使用当前 Task/Submission 版本，不要求等于提交时 Task version，并以 J-02 验证异常漂移仍可退出。

Done 重新打开后新建 reviewCycle，旧验收只保留历史。归档设置 archiveState 并保留原 lifecycle；允许归档的状态与恢复规则在 D-013 冻结。首版允许用户推进并显式验收自己的普通任务，不强制引入多角色审批系统。
