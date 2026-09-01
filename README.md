# Agent Steward

Agent Steward 是一个本地优先、面向个人开发者的 Workspace 工作管家。它首先建立 Workspace、Repository 和 Worktree 的可靠本地边界；随后在这些真实代码上下文中管理任务、产物与验收；再让人类与 AI 共享同一套执行流程，并逐步沉淀业务事实、实现快照和优化候选。

项目包含两条清晰分离的演进线：

- `taskctl` V0 已实现为本地优先的 Rust CLI，用于 Task、Session、Checkpoint、History、Session Import 和安全 Worktree 连续性；
- V1 仍处于设计阶段，描述更完整的 Workspace、Actor、Review、Daemon、TUI/GUI 与 MCP 产品架构。

## V0：taskctl

V0 实现位于 Cargo workspace，默认数据库是操作系统用户应用数据目录下的 `agent-steward/steward.db`。开发与自动化测试应始终使用 `--database` 指定隔离数据库。

```bash
cargo build --workspace
cargo test --workspace

cargo run -p taskctl -- \
  --database /tmp/agent-steward-demo.db \
  --json --input task.json \
  task create TASK-1
```

所有 mutation 使用 `--if-version` compare-and-swap；`--json` 输出稳定 envelope。Worktree 命令仅操作本地现有分支，不提供 `push`、`force`、`clean`、`reset` 或隐式 `stash`。完整合同从 [docs/v0/README.md](docs/v0/README.md) 开始阅读，AI/Agent 的调用约束见 [AGENTS.md](AGENTS.md)。

## 产品优先级

1. **Workspace 与 Repository**：先可靠识别用户在哪里工作、有哪些 Repo/Worktree，以及当前只读 Git 状态。
2. **任务与验收**：Task 归属 Workspace，并可关联 Repo/Worktree；完成必须有可复核证据。
3. **AI 执行**：AI 作为 Actor 接入同一任务模型，不另建一套任务系统。
4. **业务事实与优化**：在 Workspace 内沉淀事实、快照和映射，再从历史生成待确认的优化候选。

## 核心原则

- Workspace 是本地数据、Repository、Task 和长期事实的第一等边界。
- Repository/Worktree Registry 首期只读，不把注册、扫描或解除关联解释为 Git 写操作或文件删除。
- Task 建立在 Workspace 之上，可选关联 Repository/Worktree；AI、Git 写入和优化能力都是后续扩展。
- 本地优先；SQLite 是运行时唯一权威，Markdown 仅作可读导出。
- TUI 和 GUI 调用相同的 Command、Query 与 Event API，不维护独立状态。
- 人类、AI 和自动化统一建模为 Actor；Task Manager 管理任务池，Task Owner 推进具体任务。
- AI 声明完成只会进入 Review，验收通过后任务才能进入 Done。
- 所有 aggregate 状态变化写入统一 DomainEvent；TaskEvent 是其中的任务事件族。
- 需求、偏好、流程和 Prompt 优化只生成候选，未经用户确认不进入正式配置。

## V1 文档

从 [docs/v1/README.md](docs/v1/README.md) 开始阅读。

## 暂定组件名

| 组件 | 用途 |
|---|---|
| `taskd` | Workspace/Repo、任务状态、查询、事件和本地持久化核心 |
| `steward-tui` | 键盘优先的终端任务界面 |
| `steward-gui` | 可视化任务管理界面 |
| `stewardctl` | 脚本和自动化使用的 CLI |
| `steward-mcp` | 第三阶段供 AI 使用的 MCP Bridge |
| Runtime Adapter | 第三阶段连接不同 AI/Agent 宿主 |
| Optimization Steward | Phase 5 整理需求、偏好与优化候选 |

组件名称仍属于 V1 设计项，在公开发布前需检查 GitHub、包管理器和可执行文件名冲突。
