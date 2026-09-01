# Agent Steward 协作协议

本仓库中的 AI/Agent 在推进 V0 Task 时，应把 `taskctl` 的 SQLite 状态视为任务连续性的权威来源：

1. 开始工作时运行 `taskctl task show <task-id> --json`，读取当前 `version`；
2. 使用该版本执行 `task claim` 或 `task resume`，成功后继续携带返回的新版本；
3. 进度、决策、风险或阻塞发生实质变化时，使用对应命令写入，并从结果取得下一版本；
4. Git 现场只通过 `taskctl worktree status <task-id> --json` 或 Git 只读命令判断，不把数据库引用当作现场事实；
5. 会话结束或上下文即将耗尽前保存结构化 Checkpoint；
6. 遇到 `VERSION_CONFLICT` 时重新读取 Task，不盲目重试旧 Patch；
7. 不直接编辑 SQLite，不在 Task、Checkpoint、History 或 Session Import 中保存 Token、Cookie、密码、授权头或隐藏推理；
8. 不因测试通过、会话结束或 AI 自行判断而自动关闭 Task，关闭结果由用户明确决定。

示例（使用隔离数据库）：

```bash
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json task show TASK-1
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json task claim TASK-1 --session session-a --if-version 1
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json task checkpoint TASK-1 --session session-a --if-version 2 --input checkpoint.json
```

Worktree 的创建、删除、采纳和解除登记必须使用显式命令及最新版本；不得绕过 dirty 检查，也不得把 `taskctl` 扩展为隐式 `push`、`force`、`clean`、`reset` 或 `stash`。
