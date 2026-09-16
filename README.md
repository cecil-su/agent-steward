# Agent Steward

Agent Steward 是本地优先的任务数据中心。用户主导任务方向和状态，AI 辅助理解需求、开发、记录信息并按用户指令操作；Steward 保存和提供这些数据，不替用户判断任务是否完成。

`taskctl` 提供 CLI，`task-hook` 接收显式绑定的宿主事件元数据，`taskd` 提供 HTTP 与只读工作台。SQLite 保存业务状态。项目源码路径是普通资料，不访问源码目录、不查询 Git，也不管理 Worktree。

## 当前合同

| 项目 | 值 |
| --- | --- |
| 数据库 | SQLite Schema8，仅空库初始化；旧格式拒绝普通打开 |
| CLI/HTTP 业务 envelope | `schemaVersion:3` |
| UI API / 包格式 | 5 / 1 |
| Task 引用 | 数字 ID、`#ID`、可选且只可设置一次的 taskKey |
| Project 引用 | 数字 ID、`##ID`、唯一名称 |
| 并发保护 | Task version、Project revision、Rule revision 独立 CAS |

### 七种任务状态

`backlog` 暂不开始、`todo` 等待开始（创建默认）、`in_progress` 执行中、`in_review` 待审核或验收、`blocked` 受阻、`done` 已完成、`cancelled` 不再推进。

任意状态间均可按用户指令直接转换，没有领取、完整描述、审核、上线或阻塞理由等流程前置条件。状态通过统一 `task status` / `task-status` 入口修改；同状态在 CAS 校验后无写入。`active` 包含除 `done/cancelled` 外的状态。

状态与 Session 独立：状态变化不新建、结束、恢复 Session，不清空下一步或其他记录；claim/resume/session close 也不改变任务状态。AI 不因测试通过、交付或会话结束自动将任务设为完成。

```bash
# DB 必须是明确的隔离路径，TASK/VERSION 来自该库的最新读取。
taskctl --database DB --json task create
taskctl --database DB --json task show TASK
taskctl --database DB --json task status TASK in_review --if-version VERSION
taskctl --database DB --json task context TASK --require-read-only
```

Note 可在所有状态追加，不要求领取。正文保留原文，页面对任务描述、Note、Checkpoint 使用安全 Markdown 展示。CLI/HTTP 的输入校验、CAS、事务和 History 保留；冲突后重新读取，不自动重放旧写入。

## 数据与边界

- 默认库位于用户应用数据目录 `agent-steward/steward.db`。开发和自动化测试显式使用隔离 `--database`，不对正式任务造测试数据。
- 项目、组件、路径资料、规则、Task、Note、Session、Checkpoint、History 由数据库提供；记录路径不表示当前机器存在该目录或内容已验证。
- 不提供 Worktree 管理、Git 分支/HEAD/dirty 查询、源码文件读取、目录摘要或基于目录自动寻找任务。任务不含 Worktree/仓库关联字段。
- 旧关闭/阻塞字段和 Checkpoint gitHead 仅为历史事实，不代表当前状态，不再自动采集 Git 信息。原 History 不改写。
- 显式 Session Import 仍可在确认敏感内容后有界读取指定普通文件；迁移和宿主证据保留必要文件身份/权限检查。这不是源码上下文服务。
- SQLite 无内容级加密，不保存凭据、授权头或隐藏推理。

## 构建与验证

```bash
cargo build --workspace --locked
STEWARD_TEST_BIND=172.19.10.185 cargo test --workspace --locked
cd web
npm ci --ignore-scripts
npm run typecheck
npm test
npm run build
```

前端 Node 版本见 `.nvmrc`。React 与原生 UI 共用 Markdown 解析与样式，`npm run build:native-markdown` 只更新原生源码的生成前缀。页面保持业务只读；认证登录/退出不是任务写操作。

## 迁移与运行

Schema2/4/5/6/7 的停写快照及独立 archive-v7 格式使用显式离线导入，目标为不存在的 Schema8 新库；不原地升级，不自动切换正式库。旧 Git Source 需要显式提供源码绝对路径映射，不从 common-dir 猜测、不调用 Git、不访问保存的源码路径。详见[迁移合同](docs/v0/18-Schema7隔离导入与验证.md)。

旧状态映射：open→todo，pending_release→in_review，closed+completed→done，其他 closed 结果→cancelled；进行中/受阻保持。原关闭结果、原因、时间和 History 保留。

获准的本地服务使用指定网卡地址：

```bash
cargo run -p steward-server --bin taskd -- --database /absolute/isolated/steward.db --bind 172.19.10.185 --no-open
```

访问 `http://172.19.10.185:<端口>`。本机信任、严格认证、Origin/CSRF、Cookie 和网络边界见[HTTP合同](docs/v0/11-Hook与HTTP合同.md)。不向代理或隧道暴露免凭据本机入口。

构建不等于部署。外置原生页面、React 内嵌资源、CLI/taskd、宿主扩展与数据库必须匹配合同；安装、正式迁移、服务停启、远程写入各自需要明确授权。内嵌同步是独立操作，不自动覆盖活动发布。

## 文档

- [V0 合同](docs/v0/README.md) · [CODEMAP](CODEMAP.md) · [协作规则](AGENTS.md)
- [Web 指南](web/README.md) · [原生 UI](crates/server/web-legacy-readonly/README.md)
- [宿主接入](integrations/README.md) · [Windows 安装边界](distribution/windows/README.md)
- [V1 独立设计](docs/v1/README.md)：不是当前实现或 V0 前置条件。
