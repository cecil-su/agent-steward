# GUI 演进方案

## 1. 结论

GUI 不属于初版。Rust CLI 跑通 Task、Session、Checkpoint 和 Worktree 闭环后，可以通过 Rust 本地 Daemon 暴露相同 Application Service 和 SQLite Repository。

正式 GUI 不解析 CLI 文本，也不直接访问或修改 SQLite。短期调试页面可以消费 `taskctl --json`，但不作为稳定架构。

## 2. 推荐形态

```text
React Web GUI
      │ HTTP
      ▼
Rust Local Daemon
      │
Application Service
      ├─ SQLite Repository
      └─ Git Adapter
```

Daemon 默认只监听 loopback。CLI 和 Daemon 可以并发打开同一个本机 SQLite 数据库，由 WAL、busy timeout 和 Task version 处理竞争；Daemon 不成为数据库权威性的额外来源。是否需要 SSE/WebSocket，等 AI Client Hook 或实时会话查看出现真实需求后再决定。

## 3. GUI 页面

### 任务列表

- Task 状态、当前 Session、阻塞和下一步；
- Repository 和 Worktree 路径；
- 最近更新时间。

### 任务详情

- 目标、范围和验收条件；
- 当前状态、备注和阻塞；
- 最新 Checkpoint；
- Session 历史；
- Worktree 和实时 Git 状态；
- History 和关闭结果。

### Session 查看

- Session 来源、外部 ID 和记录路径；
- `continuedFrom` 关系；
- 来源 Checkpoint；
- 后续 Hook 实际采集到的可观察事件。

GUI 不能把 Session 结束、摘要或测试文本显示成 Task 已验收。

## 4. API 草案

```text
GET  /api/tasks
GET  /api/tasks/:id
POST /api/commands/task-create
POST /api/commands/task-claim
POST /api/commands/task-update
POST /api/commands/task-checkpoint
POST /api/commands/task-resume
POST /api/commands/task-close
GET  /api/tasks/:id/history
GET  /api/sessions/:id
GET  /api/tasks/:id/worktree-status
POST /api/commands/worktree-create
POST /api/commands/worktree-remove
```

Mutation 使用 Task version 防止覆盖更新。API 错误返回稳定 code，不依赖自然语言判断。

## 5. GUI 前置条件

开始 GUI 前必须证明：

1. Core 和 Application Service 不依赖终端；
2. CLI 人类输出与结构化结果分离；
3. SQLite Schema 和 migration 已版本化；
4. 所有数据库写入只经过 Application Service，并具有事务和并发测试；
5. Worktree 状态由 Git 实时提供；
6. 手工 CLI 流程在没有 Hook 时可以完整工作。
