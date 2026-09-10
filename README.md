# Agent Steward

Agent Steward 是一个本地优先、面向个人开发者的 Workspace 工作管家。它首先建立 Workspace、Repository 和 Worktree 的可靠本地边界；随后在这些真实代码上下文中管理任务、产物与验收；再让人类与 AI 共享同一套执行流程，并逐步沉淀业务事实、实现快照和优化候选。

上述 Workspace 路线属于 V1 设计表述，不是 V0 的实现前置条件。项目当前同时保存彼此独立的 V0 实现与 V1 设计：

- V0 已提供 `taskctl` 本地 Rust CLI、`task-hook` 通用元数据 Hook 和 `taskd` 本地 HTTP/GUI；覆盖 Task、Session、Checkpoint、History、Session Import、安全 Worktree 和会话观测；
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

Task 使用数据库自动生成且不复用的数字 ID；人类界面显示为 `#12`。可选 `taskKey`（例如 `TASK-1`）只可设置一次。后续命令接受 `12`、`#12` 或 `taskKey` 作为 Task 引用。`task create` 可以不带参数创建最小 Task，也可以通过 `--input <file>` 或 `--json --input -` 读取完整 UTF-8 JSON。描述字段最初可为 `null`，设置为字符串后不能清空。非空标题统一使用 `MMDD｜类型｜主题`；`task retitle` 可在不重新打开 Task 的前提下修正已关闭任务标题。

Task 列表支持 status、taskKey 和 title/goal/scope 文本筛选、固定长度筛选摘要游标分页及字段投影；终端默认显示表格，单字段可使用 `--format lines`。当前 V0 默认只初始化新数据库，不提供隐式旧 schema 升级或旧 Key 转义兼容。旧 v7 已关闭任务可通过显式离线 [归档迁移](docs/v0/13-v7归档迁移.md) 导入不存在的新库，保留原库不变。Schema 2 停写快照可通过独立的 [`database import-schema2`](docs/v0/18-Schema2到4迁移与隔离演练.md) 复制到私有新库，保留活动任务/会话/Hook 墓碑，不安装或切换正式服务。除创建外，所有面向已有 Task 的 mutation 使用 `--if-version` compare-and-swap；`--json` 输出 `schemaVersion: 2` 稳定 envelope。Worktree 命令仅操作本地现有分支，不提供 `push`、`force`、`clean`、`reset` 或隐式 `stash`。完整合同从 [docs/v0/README.md](docs/v0/README.md) 开始阅读，AI/Agent 的调用约束见 [AGENTS.md](AGENTS.md)。

## V0 日常使用

```bash
# 在当前 Worktree 或子目录找到关联任务；在仓库根目录查看候选
taskctl task here

# 日常工作视图：active 为未关闭任务，recent 为所有任务按最近更新排序
taskctl task list --view active
taskctl task list --view in-progress
taskctl task list --view blocked
taskctl task list --view recent

# 输出可直接交给另一个窗口的上下文，不创建 Session、不领取任务
taskctl task context 12 --format markdown > task-context.md
taskctl --json task context 12
```

开发和测试请在命令中添加 `--database <隔离数据库路径>`。`task here` 只列出候选，不自动选择或领取；优先匹配当前 Worktree，未匹配时按 Git common-dir 查找同仓库的已登记任务。未绑定 Worktree 的任务可通过列表找到。

`task context` 输出目标、范围、验收条件、最新 Checkpoint、下一步、阻塞、风险、当前 Session 和实时 Git 状态；不会自动附带导入的聊天原文。Git 观察失败时仍返回任务上下文，并给出警告。`resume` 保留原有 CAS/Session 行为，终端输出采用同样的分段摘要；`--json` 保留结构化输出。

## V0 本地工作台与 Hook

```bash
cargo build --workspace --locked
cargo run -p steward-server --bin taskd -- --database /tmp/steward-m5-demo/steward.db
```

启动后默认自动打开 `http://127.0.0.1:43123`。本机直接访问免凭据，包括通过 `--bind` 指定的本机网卡 IP 访问；严格认证模式用 `--require-local-auth`。本机免登录信任所有本地用户/程序，不可通过代理或隧道暴露。远程/严格模式的浏览器授权保留 30 天。`--no-open` 可用于无桌面环境，`--port 0` 可选临时端口；其他设备首次使用终端显示的只读凭据。Windows 本地编译更新可双击 [`distribution/windows/Update-Local.cmd`](distribution/windows/Update-Local.cmd)，首次设置后保留 IP、端口和数据路径；不自动拉代码，编译与数据库版本预检成功后才切换受管服务；现有 Schema 2/4 安装须经独立迁移并使用新的安装目录，不由普通更新跨 Schema 切换。详见 [启动与更新指南](distribution/windows/README.md)。首次创建的私有凭据跨重启保留，SSE 同步 CLI/Hook 变更，保留未应用搜索和复制输入。Web 为只读工作台，支持任务/项目检索、Checkpoint、Session、History、代码现场和项目资料/来源查看与上下文复制，不提供业务写入口；业务维护由 CLI 完成。已安装程序无需前端依赖；前端开发与内嵌同步见 [web/README.md](web/README.md)。`task-hook` 接收显式配置宿主的 JSON 事件，只保存种类和时间等元数据，不存消息或工具正文。

当前数据库格式为 **schema 5**，不自动迁移旧 schema 1/2/3/4；CLI/HTTP JSON envelope 仍为 `schemaVersion: 2`。Schema4 可通过显式 `database import-schema4` 复制到私有新库，资料使用项目 revision 与来源任务 version 校验。2026-09-09 已按用户授权完成本机成套迁移/发布，详情与恢复边界见 [Schema5 项目资料与发布记录](docs/v0/19-Schema5项目资料与CLI维护.md)。其它环境仍须单独停写、备份、迁移、核验和授权，不能通过普通更新跨 schema。

## V0 项目与任务归属

Project 提供唯一名称和稳定数字 ID，人类引用为 `##3`，Task 仍为 `#34`。项目创建/查询/改名、按项目创建/筛选任务及 CAS 关联已实现；不自动绑定目录、领取任务或采纳 Worktree。

```bash
taskctl --database /absolute/demo.db project create --name Mailroom
taskctl --database /absolute/demo.db task create --project Mailroom
taskctl --database /absolute/demo.db task list --project '##1'
```

示例编号取决于创建返回值。`task context` 增加同事务项目信息及 Checkpoint 之后的新 Notes，超过 50 条显式提示读取完整 Notes。组件/源码根、多仓/monorepo 关联、`project here` 候选定位及 `task components` 范围选择已实现，Git 源须指定 Worktree 解析，所有操作不自动领取或采纳。`project context` 已提供现场来源导航、显式文件片段与 compact JSON data 字节预算；增加前后 Git HEAD/分支/dirty 证据与明确复用阻塞原因，但不持久缓存、不证明完整依赖或宿主环境，`reuseAllowed=false`。项目管理 CLI/HTTP 写合同保留；React 页面仅只读检索和展示，可按项目筛选任务。项目资料由 `project profile set` 维护，记录来源任务/依据与 before/after History，项目详情和任务上下文均可读取。缓存与 Pi 桥接暂缓，不作为项目功能交付前提。人工流程见 [项目管理验收](docs/v0/17-项目管理验收.md)；合同、限制和阶段计划见 [项目与上下文复用](docs/v0/16-项目与上下文复用.md)。

安装、绑定、事件输入、HTTP 合同和测试方法见 [M4/M5 使用与验收](docs/v0/12-M4-M5使用与验收.md)。

## V1 设计中的产品优先级

1. **Workspace 与 Repository**：先可靠识别用户在哪里工作、有哪些 Repo/Worktree，以及当前只读 Git 状态。
2. **任务与验收**：Task 归属 Workspace，并可关联 Repo/Worktree；完成必须有可复核证据。
3. **AI 执行**：AI 作为 Actor 接入同一任务模型，不另建一套任务系统。
4. **业务事实与优化**：在 Workspace 内沉淀事实、快照和映射，再从历史生成待确认的优化候选。

## V1 设计中的核心原则

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

## V1 暂定组件名

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

Codex 与 pi 的原生 Hook 配置见 [客户端适配指南](integrations/README.md)。
