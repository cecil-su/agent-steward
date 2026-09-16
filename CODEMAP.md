# Agent Steward Code Map

当前实现是数据中心式 V0：Schema8、业务 JSON envelope3、UI API5、包格式1。V1 文档是独立设计，不代表已实现能力。

## 模块与依赖

Cargo workspace 有五个 crate。CLI / HTTP 调用 Application Service，Application 使用 core DTO 与 storage-sqlite，并直接通过 rusqlite 执行事务。

| 路径 | 职责 |
| --- | --- |
| `crates/core/src/lib.rs` | 七状态、Task/Session/Note/Checkpoint/History DTO，跨平台私有权限 |
| `crates/core/src/sources.rs` | 组件、普通源码路径资料及纯文本校验，不访问路径 |
| `crates/core/src/host_evidence.rs` | 宿主证据格式、绑定与有效期校验，不是身份认证或执行授权 |
| `crates/storage-sqlite/src/lib.rs` | Schema8、空库初始化、WAL、busy timeout、外键、写事务与版本拒绝 |
| `crates/application/src/tasks.rs` | Task 创建、查询、信息维护、通用状态设置、Note、显式 Session 领取和 Checkpoint |
| `crates/application/src/sessions.rs` | 只读数据上下文、Session attach/resume/close、显式文件 Import、History、数据库诊断 |
| `crates/application/src/projects.rs` | 项目、任务归属及独立 Project revision |
| `crates/application/src/sources.rs` | 组件及源码路径资料 CRUD，任务组件选择，不做仓库识别或源码扫描 |
| `crates/application/src/project_profiles.rs` | 有来源任务依据的项目资料维护 |
| `crates/application/src/rules.rs` | 个人/项目规则、独立 revision、完整有效规则上下文 |
| `crates/application/src/hooks.rs` | 显式 Session 绑定、事件元数据去重、删除墓碑、容量限制 |
| `crates/application/src/db.rs` | 数字/#ID/taskKey 解析、行解码、CAS、History |
| `crates/application/src/path_safety.rs` | 必要的文件身份、路径规范化、私有迁移路径安全；无 Git 调用 |
| `crates/application/src/host_context.rs` | 从 Task/Session 与已存项目源码资料派生绑定，不读取源码现场 |
| `crates/application/src/host_evidence.rs` | 对显式宿主报告及规则证据进行校验，不选择或恢复任务 |
| `crates/application/src/migration.rs`、`migration/` | 冻结源格式只读导入、明确状态/字段转换、安全发布新库 |
| `crates/cli/src/main.rs` | Clap、统一状态命令、JSON envelope3、人类输出、显式输入确认 |
| `crates/cli/src/bin/task-hook.rs` | 宿主事件投影，不保存消息/工具正文，不推断业务状态 |
| `crates/server/src/lib.rs`、`projects.rs` | HTTP 路由、严格 DTO、权限/CAS/Origin/CSRF，读写并发槽 |
| `crates/server/src/ui.rs` | 合同5外置包/内嵌回退、三文件包与摘要校验 |
| `crates/server/src/{credentials,browser_auth,events}.rs` | 本机授权、Cookie、凭据、SSE 数据失效通知 |
| `web/src/` | React 只读页面；Task/Project 数据查询与安全 Markdown |
| `web/src/lib/markdown.ts`、`web/src/markdown.css` | 两套页面共用解析、清洗和样式 |
| `crates/server/web-legacy-readonly/` | 原生只读页面源码，生成的 Markdown 前缀可重建 |
| `crates/server/web-readonly/` | 显式同步的 React 内嵌资源，不由普通构建自动覆盖 |
| `integrations/` | 宿主观测与任务适配候选，安装版本须独立核实 |
| `distribution/windows/` | 包版本门禁、进程身份、独立安装/更新/UI 激活，不负责自动迁移 |

不存在 Git 适配层、Worktree 管理模块或源码现场读取服务。用户和 AI 使用开发工具检查实际代码，Steward 只保存相关资料。

## 核心调用链

### 任务状态

`task status` / `task-status` → 校验七种合法值 → BEGIN IMMEDIATE → 读取 Task/CAS → 同值 no-op 或更新 status/version/updatedAt → 同事务写 `task.status_changed` → 返回事务快照。

状态之间没有流程门槛，也不创建/结束/恢复 Session、不清空下一步或历史字段。创建默认 todo；active 排除 done/cancelled。

### Session 与 Note

显式 claim/attach/resume/session close 只维护执行会话，保留归属、未结束状态、明确续接来源及接管保护，不改变任务状态。Checkpoint 要求有效的当前 Session，但不限制业务状态。Note 不要求领取，所有状态均可追加。

### 数据上下文

`task context --require-read-only` 通过只读数据库连接返回 Task、Project、资料、规则、最新 Checkpoint、Session 和检查点之后的 Notes；不会初始化缺失库，不观察 Git 或源码文件。Notes 最多取最近50条，截断明确提示。

### 显式文件 Import

用户确认敏感内容 → 校验指定普通文件与有界读取 → SHA-256 → Task CAS / Session 归属 → BLOB 去重存储 + History → 同事务提交。它是独立的显式导入，不是自动采集或源码导航。

### 离线数据库迁移

确认与停写快照 → 源库只读/格式及完整性检查 → 私有暂存新库 → 明确状态与 Source 路径映射 → 逐字段转换核对/外键/序列 → 复查源与目标父目录身份/版本 → 不覆盖发布。不会读取历史记录中的源码路径，不自动切换程序或库。

## 数据不变量

- Task ID 不复用；taskKey 只可首次设置；任务标题符合 `MMDD｜类型｜主题`。
- 已有 Task mutation 使用版本 CAS；项目和规则各自有 revision。失败不产生部分数据或历史。
- 描述可增量填写；已设置描述不清空。描述完整性不是任何状态的前置条件。
- 当前 Session 必须属于该 Task 且未结束；Checkpoint 必须属于同一 Task 的 Session。
- 历史关闭/阻塞字段、Checkpoint gitHead 不驱动当前业务状态；旧 History payload 保留原文。
- Schema8 不含任务四个 repository/worktree 字段、repositories 表和源码目录身份字段；SourceRoot 仅记录目录文本与项目/组件归属。
- 源码资料不是现场验证、目录权限或执行授权；不通过资料路径提供文件正文。
- HTTP 保持角色、精确 Host/Origin、CSRF、CSP、读写容量及取消后 worker 持有 permit 的约束。

## 测试导航

- Application `task_status.rs`：49种状态组合、无流程门槛、CAS/no-op、历史、并发与故障回滚。
- Application `task_sessions.rs`、`task_notes.rs`、`v0_flow.rs`：Session 解耦、终态 Note、Checkpoint/Import/上下文。
- Application `projects.rs`、`sources.rs`、`project_profiles.rs`、`rules.rs`：归属、纯路径资料、独立CAS、来源、历史。
- Application `migration/` 与各入口 migration/schema 测试：旧格式显式转换、源只读、安全发布、保留数据与序列。
- `host_context_tests.rs`、`host_evidence_pin_tests.rs`：已存资料绑定、规则文件身份、过期/替换拒绝；Linux pin 须 Linux 执行。
- CLI/Server contract、projects、rules、task_maintenance、认证/UI 包测试：入口一致性与安全边界。
- `web/` Vitest、构建门禁、`scripts/markdown-smoke.mjs`：只读页面、Markdown 安全及窄屏；浏览器脚本不替代业务验收。

```bash
cargo check --workspace --locked
cargo test --workspace --locked
cd web
npm run typecheck
npm test
npm run build
```

运行状态、部署身份与验收结果必须实际核实；源码、构建和测试不证明正式实例已升级。合同见 [V0 文档](docs/v0/README.md)，授权与协作约束见 [AGENTS.md](AGENTS.md)。
