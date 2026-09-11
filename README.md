# Agent Steward

Agent Steward V0是本地优先的任务上下文工具。`taskctl`维护Task、Project、Session、Checkpoint、资料和规则；`task-hook`投影显式绑定的客户端事件元数据；`taskd`提供HTTP接口和只读Web工作台。SQLite保存业务状态，Git和文件系统提供实时现场。

## 版本与方案关系

V0实现与[V1独立设计](docs/v1/README.md)分别管理。V1的Workspace、Actor、Review、TUI/GUI和MCP架构不是V0实现前置条件，也不因名称或编号形成自动继承、兼容或迁移关系。本文运行命令只针对V0，不把V1设计列为已实现能力。

## 当前合同

| 项目 | 值 |
| --- | --- |
| 本机业务数据库 | SQLite Schema7，仅空库初始化；不兼容库拒绝普通打开 |
| CLI/HTTP业务JSON | envelope `schemaVersion:2` |
| UI API / 包格式 | 4 / 1 |
| Task引用 | 数字ID、`#数字`或可选且只可设置一次的taskKey |
| Project引用 | 数字ID、`##数字`或唯一名称 |
| 并发维护 | Task version、Project revision、Rule revision分别执行CAS |

默认数据库为操作系统用户应用数据目录中的`agent-steward/steward.db`。开发与测试每次显式指定隔离`--database`，不得使用默认库。数据库没有内容级加密，不保存凭据或隐藏推理。

## 构建与隔离使用

```bash
cargo build --workspace --locked
cargo test --workspace --locked

cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json task create
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json --input task.json task create TASK-1
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db --json task list --query 登录 --page-size 20
cargo run -p taskctl -- --database /tmp/agent-steward-demo.db project create --name Mailroom
```

Task可最小创建，描述初始允许null，设置后不能清空。非空标题使用`MMDD｜类型｜主题`。已有任务维护携带刚读取的version，确认和依据遵循对应命令；冲突后重新读取判断，不自动重放。

```bash
# 以下DB和TASK必须替换为获准的隔离库与其真实任务引用。
taskctl --database DB task here
taskctl --database DB task list --view active
taskctl --database DB task list --view pending-release
taskctl --database DB --json task context TASK --require-read-only
```

`here`返回候选，不自动选择或领取；`context`在专用只读连接读取任务、项目、Checkpoint、Notes和完整有效规则，再观察Git，不初始化数据库或附带Import正文。Worktree命令只使用本地已有分支，不提供push/force/clean/reset或隐式stash。关闭由用户明确决定，测试、Session结束及Hook事件不等于业务验收。

## 本地工作台与宿主接入

```bash
cargo run -p steward-server --bin taskd -- --database /tmp/steward-service-demo/steward.db
```

默认打开`http://127.0.0.1:43123`。本机直接访问免凭据，包括本机监听网卡IP；此模式信任本机用户与程序，不得经代理或隧道暴露。需要隔离本机调用时使用`--require-local-auth`。`--no-open`不打开浏览器，`--port 0`使用临时端口，`--bind`指定本机IPv4。远程/严格模式需要授权，浏览器Cookie有效30天；非回环HTTP不提供传输加密。

Web提供任务/项目检索、Checkpoint、Session、History、实时现场、项目资料、规则与上下文复制，所有角色均无业务写入口。管理员HTTP仍具备获准的业务维护和服务端文件导入能力，权限及网络风险见[HTTP合同](docs/v0/11-Hook与HTTP合同.md)。

React源码为`web/src`，内嵌快照为`crates/server/web-readonly`，UiStore可托管兼容外置包；实际活动版本须查询目标实例。构建/预览见[Web指南](web/README.md)，发布见[UI合同](docs/v0/14-UI独立发布.md)。已安装程序不需要前端运行依赖。

`task-hook`只记录实际提供的种类、时间等元数据，不存消息/工具正文、不自动改变任务执行状态。Codex/pi配置与显式绑定见[宿主适配指南](integrations/README.md)。

## 导入与安装边界

当前CLI支持`database import-schema2`、`import-schema4`、`import-schema5`、`import-schema6`和`import-v7`，没有`import-schema3`。前四者接受对应冻结Schema快照；import-v7接受schema_migrations格式的闭合归档。目标均为Schema7，不可把命令名或源版本改成目标版本。

导入只读源并发布到不存在的私有新库，不原地升级、不切换默认路径或服务。停写、WAL一致备份、校验及恢复限制见[Schema7隔离导入与验证](docs/v0/18-Schema7隔离导入与验证.md)。

Windows入口及配置见[启动与更新指南](distribution/windows/README.md)。普通更新不跨Schema，编译与版本预检不能替代迁移或正式切换验收。安装、部署、服务停启、正式数据与远程写入分别确认；源码版本不证明运行实例身份。

## 文档

- [V0合同与操作入口](docs/v0/README.md)
- [CODEMAP](CODEMAP.md)
- [Agent协作约束](AGENTS.md)
- [项目与上下文](docs/v0/16-项目与上下文复用.md)
- [项目资料维护](docs/v0/19-项目资料与CLI维护.md)
- [项目管理人工验收](docs/v0/17-项目管理验收.md)
- [V1独立设计](docs/v1/README.md)
