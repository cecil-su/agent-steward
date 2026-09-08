# GUI 演进方案

> M4/M5 已有实现；当前服务合同见 [实施合同](11-M4-M5实施合同.md) 与 [使用指南](12-M4-M5使用与验收.md)。文中 M1–M3 指早期 CLI 范围，不能将历史验收记录理解为当前全部功能已验收。


## 1. 结论

GUI 不属于 M1–M3 初版。M5 的 `taskd` 已通过本地 HTTP 暴露相同 Application Service，并内嵌静态 GUI。为保持 V0 简洁，使用原生 DOM 而非另设前端框架和构建链。

正式 GUI 不解析 CLI 文本，也不直接访问或修改 SQLite。短期调试页面可以消费 `taskctl --json`，但不作为稳定架构。

## 2. 推荐形态

```text
内嵌静态 GUI（原生 DOM）
      │ HTTP
      ▼
Rust Local Daemon
      │
Application Service
      ├─ SQLite Repository
      └─ Git Adapter
```

Daemon 默认绑定 127.0.0.1:43123，可显式绑定本机 IPv4。默认本机直连免凭据；远程/严格模式通过长期凭据或 30 天浏览器 Cookie 授权，管理员/只读凭据在私有目录跨重启保留。HTTP 保持精确 Host/Origin、CSRF 与角色检查。网络信任与凭据完整合同统一见 [使用指南](12-M4-M5使用与验收.md)，不再采用早期每次启动新秘密和仅页面内存登录方案。

CLI 和 Daemon 可以并发打开同一个本机 SQLite 数据库，由 WAL、busy timeout 和调用方携带的 Task expected version 处理竞争；Daemon 不成为数据库权威性的额外来源。当前通过 SSE 通知已提交的数据库变化，客户端重新读取快照；外部 Git 文件变化仍需主动刷新，不提供持久事件重放。

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
- M4 Hook 实际采集到的元数据事件。

GUI 不能把 Session 结束、摘要或测试文本显示成 Task 已验收。

## 4. 业务 API 索引

```text
GET  /api/tasks?status=&taskKey=&query=&pageSize=&cursor=&fields=
GET  /api/tasks/:id
GET  /api/tasks/:id/context
GET  /api/tasks/:id/notes
POST /api/commands/task-create
POST /api/commands/task-claim
POST /api/commands/task-update
POST /api/commands/task-retitle
POST /api/commands/task-note
POST /api/commands/task-block
POST /api/commands/task-unblock
POST /api/commands/task-checkpoint
POST /api/commands/task-resume
POST /api/commands/task-close
GET  /api/tasks/:id/history
GET  /api/sessions/:id
GET  /api/sessions?taskId=:id
POST /api/commands/session-bind
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
POST /api/hook
GET  /api/sessions/:id/events?after=&limit=
POST /api/commands/hook-clear
GET  /api/doctor
```

Task 列表 Query 与 CLI 使用相同的筛选、游标分页和字段白名单投影合同，默认返回完整 TaskView；GUI 不解析 CLI table/lines 文本。Task/Session/Import/Worktree 显式 mutation 中，除 `task-create` 外都必须携带调用方最近查询得到的 `expectedVersion`，成功后返回完整 Task 和新 version；冲突返回 `VERSION_CONFLICT` 及 expected/current version。API 复用 CLI 文档中的 `schemaVersion=2` envelope、整数 Task ID/`taskId` 和稳定 code，不依赖自然语言判断。

观测事件 `/api/hook` 独立去重，不使用 Task expectedVersion，也不返回伪造的新 Task version；`hook-clear` 属于显式 CAS mutation。POST 使用 JSON；身份可来自可信本机直连、请求头凭据或 Cookie，本机/Cookie 写入还需精确 Origin 与 CSRF 头。不自动重试写请求。连接授权、SSE 和 UI 包接口见使用指南及 [UI 独立发布](14-UI独立发布.md)。详细 DTO 和操作确认规则见实施合同及使用指南。

## 5. GUI 前置条件

开始 GUI 前必须证明：

1. Core 和 Application Service 不依赖终端；
2. CLI 人类输出与结构化结果分离；
3. SQLite 使用单一 Schema 和 `user_version` 标识；
4. 所有数据库写入只经过 Application Service，并具有事务和并发测试；
5. Worktree 状态由 Git 实时提供；
6. 手工 CLI 流程在没有 Hook 时可以完整工作；
7. Daemon 的认证、Host/Origin 校验、CORS 默认拒绝和 mutation CSRF 测试全部通过。
