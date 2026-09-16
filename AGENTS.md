# Agent Steward 协作协议

本仓库的任务数据以 `taskctl` 本机 SQLite 状态为准。Steward 是用户主导的数据中心，AI 辅助理解、开发和记录，按用户明确指令操作业务状态。

1. 已关联任务或用户要求管理任务时，先 `task show <task-id> --json` 或只读 `task context` 取得当前版本；普通咨询不自动创建或关联任务。
2. 任务有 `backlog/todo/in_progress/in_review/blocked/done/cancelled` 七状态，创建默认 `todo`。只有 `task status <task-id> <status> --if-version <version>` 改变业务状态；任意状态可直接转换，不要求先领取、开发、审核或上线。
3. 测试通过、交付、Session 结束及 Hook 事件均不自动改变任务状态。只有用户明确选择的目标状态才可写入，包括 done/cancelled 及从终态返回其他状态。
4. 不隐式 claim、take-over 或 resume。显式 claim/resume 仅管理 Session 关联，不改变业务状态；status 不创建、结束、恢复 Session，也不清空已有记录。历史 Session 不自动重新激活。
5. 每次 Task mutation 携带最新 `version`；Project/Rule 使用各自 `revision`。业务变更与 History 同事务保存。遇到 `VERSION_CONFLICT` 重新读取并判断，不盲目重放旧输入。
6. update/project/components 需要用户针对任务、已观察版本及字段补丁的明确确认，并提供依据；不得以一般执行权限代替字段修改授权。title-only 可用 retitle，不改变状态或 Session。
7. 非空标题使用 `MMDD｜类型｜主题`，日期按 Session 时间转换到 `Asia/Shanghai`，类型为功能、设计、修复、优化、发布、探索、文档或研究。
8. Note 可用于全部状态，无需先领取。需要保存 Checkpoint 时使用同任务尚未结束的当前 Session；没有当前 Session 时不得为写检查点而隐式领取或恢复。
9. 不直接编辑业务 SQLite，不在 Task、Note、Checkpoint、History 或 Session Import 保存 Token、Cookie、密码、授权头或隐藏推理。
10. Steward 不管理 Worktree，不主动查询 Git，不读取项目源码。登记源码路径仅是普通资料，不证明路径存在、仓库身份或执行权限。编码代理需要核实 Git/源码时使用独立工具，保留用户工作树现场。
11. 测试仅使用隔离数据库和合成数据；不使用正式任务造测试数据，不自动部署、迁移正式库、提交、远程写入或切换服务。

当前源码合同：Schema8、CLI/HTTP envelope3、UI合同5、包格式1。旧库仅通过显式离线复制迁移，普通打开不升级。旧 History 和关闭结果含义保留，不从旧 Session 恢复执行权。

隔离示例（TASK/VERSION 必须取自明确指定的测试库）：

```text
taskctl --database TEST_DB --json task show TASK
taskctl --database TEST_DB --json task status TASK in_review --if-version VERSION
```

完整合同见 [V0 文档](docs/v0/README.md)。
