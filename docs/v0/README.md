# Agent Steward V0 文档

> 状态：历史合同与可运行参考实现、持续勘误，不是当前 V1 产品设计依据。当前产品设计请阅读 [V1 文档](../v1/README.md)，V0 修订记录见 [CHANGELOG](CHANGELOG.md)。

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
- SQLite 由 CLI 进程直接访问，不依赖独立数据库服务，也不支持多台机器共享同一数据库；
- Git 和文件系统实时状态不写成数据库权威事实；
- 所有 Task mutation 携带调用方最近读取的 version，使用 compare-and-swap 防止旧快照覆盖；
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
- Cargo workspace 中的 `taskctl` 是本合同的可运行参考实现，用于验证 M1–M3 的本地 CLI 闭环；它不提前实现 V1 的 Daemon、TUI/GUI、MCP 或产品领域模型。
