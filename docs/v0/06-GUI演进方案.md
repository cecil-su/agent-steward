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

Daemon 默认只监听 loopback，但 loopback 不是完整的浏览器安全边界。每次启动必须生成不可预测的认证秘密，或使用能够证明同一 OS 用户身份的等价本地认证；HTTP 还必须校验 Host 和 Origin、默认拒绝跨 Origin，并为 mutation 提供 CSRF 防护。认证秘密不得进入普通日志、History 或浏览器持久化存储。

CLI 和 Daemon 可以并发打开同一个本机 SQLite 数据库，由 WAL、busy timeout 和调用方携带的 Task expected version 处理竞争；Daemon 不成为数据库权威性的额外来源。是否需要 SSE/WebSocket，等 AI Client Hook 或实时会话查看出现真实需求后再决定。

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
- Worktree 的数据库登记路径和实时 Git 状态，二者必须分开展示；登记现场已消失时仍保留诊断与 detach 所需路径；
- History 和关闭结果。

### Session 查看

- Session 来源、外部 ID 和记录路径；
- `continuedFrom` 关系；
- 来源 Checkpoint；
- Session Import 元数据、重复提示和显式逻辑删除；GUI 不默认渲染原始 BLOB，删除前必须确认并展示“不保证取证级物理擦除”的边界；
- 后续 Hook 实际采集到的可观察事件。

GUI 不能把 Session 结束、摘要或测试文本显示成 Task 已验收。

## 4. API 草案

```text
GET  /api/tasks
GET  /api/tasks/:id
POST /api/commands/task-create
POST /api/commands/task-claim
POST /api/commands/task-update
POST /api/commands/task-note
POST /api/commands/task-block
POST /api/commands/task-unblock
POST /api/commands/task-checkpoint
POST /api/commands/task-resume
POST /api/commands/task-close
GET  /api/tasks/:id/history
GET  /api/sessions/:id
GET  /api/sessions?taskId=:id
POST /api/commands/session-attach
POST /api/commands/session-import-add
GET  /api/sessions/:id/imports
POST /api/commands/session-import-remove
POST /api/commands/session-close
GET  /api/tasks/:id/worktree-status
POST /api/commands/worktree-create
POST /api/commands/worktree-remove
POST /api/commands/worktree-adopt
POST /api/commands/worktree-detach
GET  /api/doctor
```

除 `task-create` 外，每个 Mutation 请求必须携带调用方最近查询得到的 `expectedVersion`，成功后返回完整 Task 和新 version；冲突返回 `VERSION_CONFLICT` 及 expected/current version。API 复用 CLI 文档中的 `schemaVersion=1` envelope 和稳定 code，不依赖自然语言判断。

## 5. GUI 前置条件

开始 GUI 前必须证明：

1. Core 和 Application Service 不依赖终端；
2. CLI 人类输出与结构化结果分离；
3. SQLite Schema 和 migration 已版本化；
4. 所有数据库写入只经过 Application Service，并具有事务和并发测试；
5. Worktree 状态由 Git 实时提供；
6. 手工 CLI 流程在没有 Hook 时可以完整工作；
7. Daemon 的认证、Host/Origin 校验、CORS 默认拒绝和 mutation CSRF 测试全部通过。
