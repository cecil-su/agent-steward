# AI Session 记录

## 1. 目标

Session 记录用于让 AI 在新会话或新窗口中继续同一个 Task。初版记录保存在本机 SQLite 数据库中，重点是可靠恢复，不追求自动保存完整聊天。

核心关系：

```text
Task row
├─ current_session_id ─────────► Session B
└─ latest_checkpoint_id ───────► Checkpoint

Session rows 通过 task_id 形成历史
Session B ── continued_from ──► Session A
```

## 2. 初版记录内容

每个 Session 保存：

```ts
type Session = {
  id: string
  taskId: string
  source?: string
  externalSessionId?: string
  continuedFrom?: string
  recordPath?: string
  startedAt: string
  endedAt?: string
}
```

- `id` 是 `taskctl` 的本地稳定 ID；
- `externalSessionId` 在 AI 客户端能够提供时保存；
- `recordPath` 指向用户明确提供的数据库外部原始记录；
- `continuedFrom` 表示新会话从哪个会话继续。

普通 attach 不复制每条消息或工具调用，任务连续性主要依赖数据库中的结构化 Checkpoint。只有用户明确执行 `session import add` 时，输入文件才作为不可推断 Task 状态的导入副本保存在 SQLite `session_imports` 中；用户可以查询元数据并通过 `session import remove` 逻辑删除。

## 3. AI 主动更新流程

### 开始处理

```bash
taskctl task show TASK-123 --json
taskctl task claim TASK-123 --session session-a --if-version 1
```

后续每次 mutation 都使用上一条成功结果返回的新 version；遇到 `VERSION_CONFLICT` 时必须重新 `show` 并重新判断，不能盲目重试旧输入。

### 执行期间

```bash
taskctl task update TASK-123 --if-version 2 --input task-patch.json
taskctl task note TASK-123 --if-version 3 --type decision --text "沿用现有 JWT 方案"
taskctl task block TASK-123 --if-version 4 --reason "缺少测试账号" --recovery "取得账号后继续"
taskctl worktree status TASK-123 --json
```

只在状态、关键决策、阻塞和下一步发生变化时更新，不要求每轮对话写入。

### 切换会话前

```bash
taskctl task checkpoint TASK-123 --session session-a --if-version 5 --input checkpoint.json
```

Checkpoint 至少包含：

- 当前进展摘要；
- 已完成事项；
- 已确认决策；
- 未完成事项；
- 唯一下一步；
- 风险和阻塞；
- 当前 Git HEAD，由 CLI 观察后补充。

### 新会话继续

```bash
taskctl task show TASK-123 --json
taskctl task resume TASK-123 --session session-b --from-session session-a --if-version 6 --take-over --json
```

`--from-session` 明确继续来源；如果省略则使用当前 Session。resume 目标 ID 必须尚不存在且不同于来源 Session ID。Task 存在活跃当前 Session 时必须显式 `--take-over`。新 Session 的 `continuedFrom` 指向来源 Session，旧记录保留且不会被伪造为已经结束。

`resume` 返回：

```text
Task 当前内容
+ 最新 Checkpoint
+ Session 历史和 continuedFrom
+ 实时 Worktree / Git 状态
+ 唯一下一步
```

新 Session 必须重新读取 Git 现场，不能只相信旧 Checkpoint。

## 4. 项目级 AI 协议

项目可以在 `AGENTS.md` 中约定：

```text
1. 处理 taskctl Task 前先 show，取得当前 version。
2. claim、resume 和其他 mutation 必须携带最近成功结果返回的 version。
3. 关键状态、决策和阻塞变化时调用 taskctl 更新。
4. 版本冲突时重新读取并重新判断，不盲目重试。
5. 不直接修改 SQLite 数据库。
6. Git 状态通过 taskctl worktree status 获取。
7. 会话结束或上下文即将耗尽前保存 Checkpoint。
```

这使初版不依赖特定 AI 客户端插件。

## 5. AI Client Hook / Runtime Adapter 规划

手工 CLI 闭环稳定后，增加客户端自动上报：

```text
AI Client
   │ Hook：Session 生命周期和可观察事件
   ▼
Runtime Adapter
   │ 统一、脱敏、幂等
   ▼
Application Service
   └─ SQLite Session Repository / Observable Log
```

计划采集：

- Session started、resumed、idle、closed；
- 客户端实际暴露的用户/助手消息；
- 工具调用和工具结果；
- 模型、usage 和错误信息，如果客户端提供；
- 外部 Session ID 和来源事件 ID。

约束：

- 不读取或推断隐藏 Chain of Thought；
- Token、Cookie、密码和授权头在落盘前脱敏；
- 来源事件按 ID 幂等导入；
- Hook 失败时不影响显式 CLI 更新；
- Hook 只补充 Session 记录，不能根据消息内容自动更新或关闭 Task；
- 客户端差异限制在 Runtime Adapter 内，不进入 Core。

V0 手工 `session import add` 还必须限制为不超过 16 MiB 的普通文件，有界流式读取并计算 SHA-256；目录、设备、Socket、FIFO 和超限输入全部拒绝。相同 Session 和 SHA-256 只保存一份。自动 Hook 的内容分片与更大附件不复用该入口。

完整 Observable Log、内容分片和大型附件存储只有在实际数据量出现后再设计。

## 6. CLI 合同

```bash
taskctl session list [--task <task-id>]
taskctl session show <session-id>
taskctl session attach <task-id> --session <session-id> --if-version <version> [--source <client>] [--external-session <external-id>] [--record-path <path>]
taskctl session import add <task-id> --session <session-id> --if-version <version> --file <session-file>
taskctl session import list <session-id> [--json]
taskctl session import remove <import-id> --if-version <version> [--yes]
taskctl session close <session-id> --if-version <version>
```

`task claim` 可以在同一 SQLite 事务中创建缺失的本地 Session；已存在的 ID 仅在它是该 Task 尚未结束的当前 Session 时允许 no-op，不能重新激活历史 Session。`task resume` 和 `task claim --take-over` 使用尚不存在且不同于来源的新 Session ID，在同一事务中创建 Session 并更新 Task。`session attach` 显式保存可选外部 Session ID；`session import add` 要求指定的本地 Session 已存在并属于同一 Task，来源读取该 Session 的 `source`，再保存用户明确提供的文件内容和哈希；`session attach --record-path` 对已存在文件保存规范化绝对路径，对不存在文件保存展开后的绝对弱引用。

`session import list` 只返回 Import ID、Session ID、路径、媒体类型、SHA-256、大小和导入时间，不返回内容。`session import remove` 使用 Task expected version 删除 BLOB 并写入不含原文的 History；它保证 V0 查询层面的逻辑删除，不承诺外部备份、文件系统快照或存储介质上的取证级物理擦除。

`session close` 设置 `endedAt`。如果关闭的是 Task 当前 Session，它还会在同一事务中清空 `currentSessionId` 并写入 History，但不会推断或改变 Task 状态。一个本地 Session 只属于一个 Task；`continuedFrom` 也必须指向同一 Task 的 Session。

`session list` 和 `task resume` 返回的 Session 历史统一按 `startedAt ASC, id ASC` 排序。

## 7. 初版验收

- Session A 可以在 SQLite 中领取 Task 并保存 Checkpoint；
- Session B 可以通过 `resume` 延续 Session A；
- resume 目标与来源相同、目标 ID 已存在或已结束时会被拒绝，不会生成 self-reference 或重新激活历史 Session；
- 恢复结果包含最新 Task 和实时 Git 现场；
- 没有完整聊天记录时仍能继续任务；
- Session 结束不会自动关闭 Task；
- 未提供外部 Session ID 时，调用方可以生成稳定本地 ID 并将外部 ID 留空；
- Hook 尚未实现时，手工 CLI 流程完整可用；
- 所有 mutation 使用 expected version，旧快照更新返回结构化冲突；
- Session 导入目标明确且超限或非普通文件会被拒绝；
- 重复导入不复制 BLOB，导入元数据可查询且内容可显式逻辑删除。
