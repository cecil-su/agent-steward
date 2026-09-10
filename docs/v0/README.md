# Agent Steward V0 文档

> 状态：V0 独立合同与可运行参考实现，持续勘误；V0 修订记录见 [CHANGELOG](CHANGELOG.md)。
>
> **版本关系：**V0、V1 以及未来可能出现的 V2 是独立方案范围，不是连续升级链。V1 不自动继承、替代或扩展 V0，V0 的实现也不计入 V1 完成度；只有专项文档明确声明时，二者才存在特定复用或迁移关系。统一口径见仓库根目录 [README](../../README.md#版本与方案关系)。

## 阅读顺序与合同边界

00–05 描述 Task/Session/存储/事务核心，06–10 描述界面与流程。当前 M4/M5 行为以 11/12 为详细合同；13 专门定义 v7 归档复制，14 定义独立 UI 发布；15 仅是参考建议与待验证计划；16 是 #34 项目/上下文开发合同，明确 Schema 4 隔离开发与尚未启用的复用边界；18 定义显式 Schema 2→4 复制入口、Windows 合成隔离验证及尚未执行的正式切换边界。历史测试数量不代表当前覆盖，CI 结果须按提交读取。出现冲突应依据代码和验收证据修订合同，不能仅按文档编号推断功能已完成。

## 项目概览

- [Task Control Plane 项目概览](00-项目概览.md)
- [V0 修订记录](CHANGELOG.md)

## 产品与架构

1. [产品目标与范围](01-产品目标与范围.md)
2. [总体架构](02-总体架构.md)
3. [架构与流程图](10-架构与流程图.md)

## 初版合同

4. [CLI 命令设计](03-CLI命令设计.md)
5. [数据模型与存储](04-数据模型与存储.md)
6. [安全与事务](05-安全与事务.md)
7. [AI Session 记录](09-AI-Session记录.md)

## 演进与交付

8. [GUI 演进方案](06-GUI演进方案.md)
9. [实施路线图](07-实施路线图.md)
10. [测试与验收](08-测试与验收.md)

- [M4 / M5 实施合同与验收记录](11-M4-M5实施合同.md)
- [M4 / M5 使用与验收](12-M4-M5使用与验收.md)
- [v7 归档迁移](13-v7归档迁移.md)
- [UI 独立发布](14-UI独立发布.md)
- [项目与上下文复用：开发合同与项目管理HTTP/页面](16-项目与上下文复用.md)
- [项目管理人工验收交接（本轮用户选择跳过）](17-项目管理验收.md)
- [Schema 2→4 迁移方案与隔离演练](18-Schema2到4迁移与隔离演练.md)

- [Schema5 项目资料与CLI维护](19-Schema5项目资料与CLI维护.md)
- [任务信息维护](20-任务信息维护.md)
- [待上线任务状态](21-待上线任务状态.md)

- [个人偏好与项目规则](22-个人偏好与项目规则.md)

当前源码 Schema 为7，仅初始化空库，不隐式升级。显式 `database import-schema2/import-schema4/import-schema5/import-schema6` 只读对应停写快照并发布到私有新库，不切换默认路径或推断任务状态。信息维护以20为准，待上线状态以21为准，规则、Schema7及UI合同4以22为准。不要将开发构建直接安装到正式 CLI/Hook/taskd。JSON envelope仍为2。

## 技术基线

- 初版 CLI 使用 Rust 实现，并采用 Cargo workspace 组织 Core、Application、SQLite Storage、Git Adapter 和 CLI；
- 任务数据保存在本机 SQLite 数据库中，不使用 Markdown、JSON 或 JSONL 文件作为主存储；
- Task 使用自动生成且不复用的数字主键，可选唯一 `taskKey` 只可设置一次；CLI 接受数字、`#数字` 或 `taskKey` 引用；`taskKey` 按原文解析，没有转义前缀；
- 最小 Task 可无描述创建，随后通过带 CAS 的 JSON Merge Patch 增量补全；描述一旦设置不能清空，`completed` 关闭前必须补齐；
- `--json --input -` 可安全读取 stdin UTF-8 JSON，机器输出合同为 `schemaVersion: 2`；
- Task list 支持 status/taskKey/文本筛选、固定长度筛选摘要游标分页和字段投影；人类模式提供表格及单字段 lines 输出；
- SQLite 由 CLI/Hook/Daemon 经共享 Application Service 访问，不依赖独立数据库服务，也不支持多台机器共享同一数据库；普通连接支持初始化空库或打开当前格式库，不隐式迁移旧 schema；旧 v7 闭合任务归档可使用独立的 [离线迁移命令](13-v7归档迁移.md) 复制到新库；
- Git 和文件系统实时状态不写成数据库权威事实；
- 除没有旧状态可比较的 create 外，所有面向已有 Task 的 mutation 携带调用方最近读取的 version，使用 compare-and-swap 防止旧快照覆盖；
- Git 与 SQLite 部分完成时只允许显式、非破坏性的 Worktree 引用 adopt/detach；
- Worktree 身份使用跨平台 CanonicalPath 与 Git common-dir，不使用原始路径字符串；
- Session Import 支持 SHA-256 去重、元数据查询和显式逻辑删除。

## 当前边界

- 当前核心概念包括 Project、Task、Session、Checkpoint、Worktree 和 History；Project 的唯一名称/`##ID`、独立 revision、Task projectId/componentIds、组件/源码根与项目目录候选已实现；项目事实缓存尚未实现；
- SQLite 保存任务上下文，Git 和文件系统保存源码现场事实；
- AI 在关键节点主动调用 CLI 更新，不能直接修改数据库；
- 结构化输入和 `--json` 输出使用已版本化 JSON 合同；
- M4 只自动采集元数据，完整会话正文仍仅通过显式 Import；不承诺后续必定采集完整会话；
- M1–M3 初版不实现 Owner、Assignment、Event、Operation、Approval、Daemon 或 GUI；M4/M5 在此基础上提供独立 Session 观测及本地 GUI，仍不引入通用领域 Event 或编排模型；
- Cargo workspace 中的 `taskctl` 是本合同的可运行参考实现，用于验证 V0 M1–M3 的本地 CLI 闭环；其完成度仅按 V0 合同判断，不据此推断 V1 或其它方案的 Daemon、TUI/GUI、MCP 或领域模型实现状态。

- [Xuanwu 参考与 V0 改进建议](15-Xuanwu参考与V0改进建议.md)：基于当前 V0 的取舍、记录约定与待验证旅程。
