# CLI 命令设计

## 1. 通用约定

```bash
taskctl [global-options] <domain> <action> [arguments] [options]
```

全局选项：

- `--database <path>`：覆盖默认 SQLite 数据库路径；
- `--json`：输出稳定机器合同；
- `--input <file>`：从 JSON 或 Markdown 文件读取较长输入，文件只作为本次命令输入，不成为主存储；
- `--yes`：确认 Worktree 删除等明确的本地操作；
- `--verbose`：输出诊断信息，不改变结果合同。

默认数据库位于当前操作系统的用户级应用数据目录下，文件名为 `agent-steward/steward.db`。初版不提供远程 Git 写入命令，也不连接远程数据库服务。

## 2. Task

```bash
taskctl task list [--status in_progress]
taskctl task show <task-id> [--json]
taskctl task create <task-id> --title <title> [--input <file>]
taskctl task claim <task-id> --session <session-id> [--take-over]
taskctl task update <task-id> [--status <status>] [--next-step <text>] [--input <file>]
taskctl task note <task-id> --type <decision|progress|risk> --text <text>
taskctl task block <task-id> --reason <text> --recovery <text>
taskctl task checkpoint <task-id> --session <session-id> --input <file>
taskctl task resume <task-id> --session <new-session-id> [--json]
taskctl task close <task-id> --outcome <outcome> [--reason <text>]
```

`claim` 的语义是把当前 Session 记录为 Task 的执行会话，并将未开始的 Task 推进到 `in_progress`。如果已有其他当前 Session，默认提示冲突；用户可以明确使用 `--take-over` 接管。

`resume` 会：

1. 将旧的当前 Session 保留在历史中；
2. 关联新的 Session；
3. 读取最新 Checkpoint；
4. 实时观察 Worktree 和 Git；
5. 返回恢复上下文和唯一下一步。

`checkpoint` 输入至少包含：

```json
{
  "summary": "当前进展摘要",
  "completed": [],
  "decisions": [],
  "pending": [],
  "nextStep": "下一步",
  "risks": []
}
```

## 3. Session

```bash
taskctl session list [--task <task-id>]
taskctl session show <session-id>
taskctl session attach <task-id> --session <session-id> [--source <client>] [--record-path <path>]
taskctl session import <task-id> --source <client> --file <session-file>
taskctl session close <session-id>
```

初版的 Session 只保存关联信息、来源和可选记录路径。完整聊天记录仍由 AI 客户端负责；`attach --record-path` 只在数据库中保存外部路径引用，`import` 只在用户明确提供文件时把可观察内容和哈希复制到 SQLite。

如果客户端没有暴露 Session ID，CLI 可以生成本地 Session ID，并将外部 ID 保留为空。

## 4. Worktree

```bash
taskctl worktree create <task-id> --repo <path> --branch <branch> [--path <worktree-path>]
taskctl worktree status <task-id> [--json]
taskctl worktree remove <task-id> [--yes]
```

初版直接封装 `git worktree add`、状态观察和 `git worktree remove`，不创建 Operation Plan。

最低保护：

- 创建前检查仓库、目标分支和路径；
- 状态命令实时读取 HEAD、dirty、staged 和 untracked；
- dirty Worktree 默认拒绝删除；
- 不提供隐式 force、clean、reset、stash 或 push；
- 删除成功后才清除 Task 中的 Worktree 引用。

## 5. History 与诊断

```bash
taskctl history <task-id> [--json]
taskctl doctor
```

History 与对应 mutation 在同一 SQLite 事务中写入，记录 Task 创建、领取、更新、Checkpoint、Session 恢复、Worktree 引用变化和关闭，不承担复杂安全审计。

`doctor` 至少检查数据库可打开、schema 版本受支持、外键一致性、SQLite `quick_check`、记录路径提示和已登记 Worktree 引用；它不能把数据库引用当作 Git 现场事实。

## 6. AI 会话更新协议

AI 应遵守：

1. 开始时运行 `task show`、`task claim` 或 `task resume`；
2. 状态、关键决策或阻塞变化时调用对应命令；
3. Git 现场使用 `worktree status` 获取；
4. 会话结束或上下文即将耗尽前保存 Checkpoint；
5. 不直接修改 SQLite 数据库。

这些规则可以写入项目 `AGENTS.md`。后续 Client Hook / Runtime Adapter 可以自动提交 Session 生命周期和可观察事件，但不能替代显式 Task 更新。

## 7. 退出码

- `0`：成功；
- `2`：输入或 Schema 错误；
- `4`：版本、Session 或数据库写入冲突；
- `5`：Worktree 安全检查拒绝；
- `10`：内部错误。
