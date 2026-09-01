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

普通 attach 不复制每条消息或工具调用，任务连续性主要依赖数据库中的结构化 Checkpoint。只有用户明确执行 `session import` 时，输入文件才作为不可推断 Task 状态的导入副本保存在 SQLite `session_imports` 中。

## 3. AI 主动更新流程

### 开始处理

```bash
taskctl task claim TASK-123 --session session-a
taskctl task show TASK-123 --json
```

### 执行期间

```bash
taskctl task update TASK-123 --status in_progress --next-step "修改登录接口"
taskctl task note TASK-123 --type decision --text "沿用现有 JWT 方案"
taskctl task block TASK-123 --reason "缺少测试账号" --recovery "取得账号后继续"
taskctl worktree status TASK-123 --json
```

只在状态、关键决策、阻塞和下一步发生变化时更新，不要求每轮对话写入。

### 切换会话前

```bash
taskctl task checkpoint TASK-123 --session session-a --input checkpoint.json
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
taskctl task resume TASK-123 --session session-b --json
```

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
1. 处理 taskctl Task 前先 show、claim 或 resume。
2. 关键状态、决策和阻塞变化时调用 taskctl 更新。
3. 不直接修改 SQLite 数据库。
4. Git 状态通过 taskctl worktree status 获取。
5. 会话结束或上下文即将耗尽前保存 Checkpoint。
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

完整 Observable Log、内容分片和大型附件存储只有在实际数据量出现后再设计。

## 6. CLI 合同

```bash
taskctl session list [--task <task-id>]
taskctl session show <session-id>
taskctl session attach <task-id> --session <session-id> [--source <client>] [--record-path <path>]
taskctl session import <task-id> --source <client> --file <session-file>
taskctl session close <session-id>
```

`task claim` 和 `task resume` 可以在同一 SQLite 事务中自动创建缺失的本地 Session 记录并更新 Task，避免要求用户预先执行多条命令。`session import` 将用户明确提供的文件内容、哈希和来源信息保存到数据库；`session attach --record-path` 只保存路径引用。

## 7. 初版验收

- Session A 可以在 SQLite 中领取 Task 并保存 Checkpoint；
- Session B 可以通过 `resume` 延续 Session A；
- 恢复结果包含最新 Task 和实时 Git 现场；
- 没有完整聊天记录时仍能继续任务；
- Session 结束不会自动关闭 Task；
- 未提供外部 Session ID 时可以使用本地 ID；
- Hook 尚未实现时，手工 CLI 流程完整可用。
