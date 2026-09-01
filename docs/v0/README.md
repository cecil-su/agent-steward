# Agent Steward V0 文档

> 本目录归档自 Task Control Plane 的设计文档，作为 Agent Steward 的 V0 设计基线保留。当前设计请阅读 [V1 文档](../v1/README.md)。

## 项目概览

- [Task Control Plane 项目概览](00-项目概览.md)

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
- Git 和文件系统实时状态不写成数据库权威事实。

## 当前边界

- 初版核心概念是 Task、Session、Checkpoint、Worktree 和 History；
- SQLite 保存任务上下文，Git 和文件系统保存源码现场事实；
- AI 在关键节点主动调用 CLI 更新，不能直接修改数据库；
- 完整会话自动采集由后续 AI Client Hook / Runtime Adapter 提供；
- 初版不实现 Owner、Assignment、Event、Operation、Approval、Daemon 或 GUI；
- 本阶段不创建源码或安装依赖，进入实现需要用户另行明确确认。
