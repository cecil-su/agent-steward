# Agent Steward V0 文档

> 状态：V0 独立合同与可运行参考实现，持续勘误；V0 修订记录见 [CHANGELOG](CHANGELOG.md)。
>
> **版本关系：**V0、V1 以及未来可能出现的 V2 是独立方案范围，不是连续升级链。V1 不自动继承、替代或扩展 V0，V0 的实现也不计入 V1 完成度；只有专项文档明确声明时，二者才存在特定复用或迁移关系。统一口径见仓库根目录 [README](../../README.md#版本与方案关系)。

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

## 技术基线

- 初版 CLI 使用 Rust 实现，并采用 Cargo workspace 组织 Core、Application、SQLite Storage、Git Adapter 和 CLI；
- 任务数据保存在本机 SQLite 数据库中，不使用 Markdown、JSON 或 JSONL 文件作为主存储；
- Task 使用自动生成且不复用的数字主键，可选唯一 `taskKey` 只可设置一次；CLI 接受数字、`#数字` 或 `taskKey` 引用，歧义旧 Key 可用 `key:` 显式引用；
- 最小 Task 可无描述创建，随后通过带 CAS 的 JSON Merge Patch 增量补全；描述一旦设置不能清空，`completed` 关闭前必须补齐；
- `--json --input -` 可安全读取 stdin UTF-8 JSON，机器输出合同为 `schemaVersion: 2`；
- Task list 支持 status/taskKey/文本筛选、固定长度筛选摘要游标分页和字段投影；人类模式提供表格及单字段 lines 输出；
- SQLite 由 CLI 进程直接访问，不依赖独立数据库服务，也不支持多台机器共享同一数据库；v6→v7 migration 通过旧 Task 锁 barrier 拒绝与仍活跃的 v6 Worktree operation 并行；
- Git 和文件系统实时状态不写成数据库权威事实；
- 除没有旧状态可比较的 create 外，所有面向已有 Task 的 mutation 携带调用方最近读取的 version，使用 compare-and-swap 防止旧快照覆盖；
- Git 与 SQLite 部分完成时只允许显式、非破坏性的 Worktree 引用 adopt/detach；
- Worktree 身份使用跨平台 CanonicalPath 与 Git common-dir，不使用原始路径字符串；
- Session Import 支持 SHA-256 去重、元数据查询和显式逻辑删除。

## 当前边界

- 初版核心概念是 Task、Session、Checkpoint、Worktree 和 History；
- SQLite 保存任务上下文，Git 和文件系统保存源码现场事实；
- AI 在关键节点主动调用 CLI 更新，不能直接修改数据库；
- 结构化输入和 `--json` 输出使用已版本化 JSON 合同；
- 完整会话自动采集由后续 AI Client Hook / Runtime Adapter 提供；
- 初版不实现 Owner、Assignment、Event、Operation、Approval、Daemon 或 GUI；
- Cargo workspace 中的 `taskctl` 是本合同的可运行参考实现，用于验证 V0 M1–M3 的本地 CLI 闭环；其完成度仅按 V0 合同判断，不据此推断 V1 或其它方案的 Daemon、TUI/GUI、MCP 或领域模型实现状态。
