# V0 修订记录

V0 是 Agent Steward 的历史设计参考，不是当前产品实现依据。当前设计以 [V1 文档](../v1/README.md)为准。原始内容可通过 Git 历史查看；本目录允许为消除内部矛盾、安全歧义和不可实施合同而持续勘误。

## 2026-09-01

- 冻结 Task Create/Patch、状态转换、expected version 和稳定 JSON DTO；
- 补齐 Session attach/resume/close、同 Task 复合外键和继续关系不变量；
- 增加 Worktree adopt/detach 以及 Git/SQLite 部分完成恢复合同；
- 定义跨平台 CanonicalPath、RepositoryIdentity 和 TOCTOU 复核规则；
- 将 Session Import 明确为 add/list/remove，增加 SHA-256 去重、逻辑删除和物理擦除边界；
- 冻结 History change type、最低 payload、错误码和警告码；
- 明确 claim/resume 只创建全新目标 Session，禁止 self-reference 和历史 Session 重激活，并固定 Session 输出排序；
- 禁止 `worktree create` 覆盖既有引用，以按 Task advisory lock 串行化外部操作，并用数据库唯一索引阻止多个 Task 登记同一路径；
- 要求 Git 任意退出状态后重观测现场，增加 `GIT_COMMAND_FAILED`，并将无法排除外部变化的结果归入 `PARTIAL_EXTERNAL_STATE`；
- 增加本地数据库权限以及未来 Daemon 的认证、Origin 和 CSRF 边界。
