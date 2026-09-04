# Agent Steward

Agent Steward 是一个本地优先、面向个人开发者的 Workspace 工作管家。它首先建立 Workspace、Repository 和 Worktree 的可靠本地边界；随后在这些真实代码上下文中管理任务、产物与验收；再让人类与 AI 共享同一套执行流程，并逐步沉淀业务事实、实现快照和优化候选。

项目当前同时保存彼此独立的 V0 实现与 V1 设计：

- `taskctl` V0 已实现为本地优先的 Rust CLI，用于 Task、Session、Checkpoint、History、Session Import 和安全 Worktree 连续性；
- V1 仍处于设计阶段，描述 Workspace、Actor、Review、Daemon、TUI/GUI 与 MCP 产品架构。

## 版本与方案关系

V0、V1 以及未来可能出现的 V2 是独立的方案/文档范围，不是语义化版本的连续升级链，也不因编号自动形成继承、替代、兼容或迁移关系。某个版本已经设计或实现的能力，不能据此认定另一个版本也已具备；进度、范围和完成度必须在各自文档与代码边界内单独判断。只有专项设计或迁移文档明确声明时，才可认为两个版本之间存在特定复用或迁移关系。

讨论需求、缺陷、路线图或验收结果时必须明确版本，不使用“旧版已有，所以新版默认继承”或“新版规划，所以旧版后续会实现”等跨版本推断。

## V0：taskctl

V0 实现位于 Cargo workspace，默认数据库是操作系统用户应用数据目录下的 `agent-steward/steward.db`。开发与自动化测试应始终使用 `--database` 指定隔离数据库。

```bash
cargo build --workspace
cargo test --workspace

cargo run -p taskctl -- \
  --database /tmp/agent-steward-demo.db \
  --json task create

cargo run -p taskctl -- \
  --database /tmp/agent-steward-demo.db \
  --json --input task.json \
  task create TASK-1

cargo run -p taskctl -- \
  --database /tmp/agent-steward-demo.db \
  task list --status open --fields id,title

cargo run -p taskctl -- \
  --database /tmp/agent-steward-demo.db \
  --json task list --query 登录 --page-size 20
```

Task 使用数据库自动生成且不复用的数字 ID；人类界面显示为 `#12`。可选 `taskKey`（例如 `TASK-1`）只可设置一次。后续命令接受 `12`、`#12` 或 `taskKey` 作为 Task 引用；迁移前恰好为 `12`、`#12` 等歧义形式的旧 Key 使用显式 `key:12`、`key:#12`。`task create` 可以不带参数创建最小 Task，也可以通过 `--input <file>` 或 `--json --input -` 读取完整 UTF-8 JSON。描述字段最初可为 `null`，设置为字符串后不能清空。非空标题统一使用 `MMDD｜类型｜主题`；`task retitle` 可在不重新打开 Task 的前提下修正已关闭任务标题。

Task 列表支持 status、taskKey 和 title/goal/scope 文本筛选、固定长度筛选摘要游标分页及字段投影；终端默认显示表格，单字段可使用 `--format lines`。v6→v7 升级前应停止所有 v6 进程；若仍有 v6 Worktree operation 持有旧 Task ID 锁，v7 会拒绝 migration 而不是并行执行 Git mutation。除创建外，所有面向已有 Task 的 mutation 使用 `--if-version` compare-and-swap；`--json` 输出 `schemaVersion: 2` 稳定 envelope。Worktree 命令仅操作本地现有分支，不提供 `push`、`force`、`clean`、`reset` 或隐式 `stash`。完整合同从 [docs/v0/README.md](docs/v0/README.md) 开始阅读，AI/Agent 的调用约束见 [AGENTS.md](AGENTS.md)。

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

## 设计文档

- [V1 独立设计方案](docs/v1/README.md)
- [V0 `taskctl` 合同与参考实现](docs/v0/README.md)

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
