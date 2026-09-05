# V0 修订记录

## 当前：精简 V0（不兼容历史数据库）

- 七版 migration 合并为单一 Schema，使用 `PRAGMA user_version=1`，仅初始化空数据库；删除旧 ID 映射、旧进程锁屏障及 `key:` 转义语法；
- 普通查询不再获取 migration 写锁；Worktree 创建失败后的恢复建议在事务释放后生成；
- 删除持久化 Worktree 路径诊断键和文件系统大小写/Unicode 探测；存在路径比较对象身份，缺失路径使用规范化精确匹配，支持直接创建中文 Worktree；
- 保留 CAS、同事务 History、任务级 Worktree 锁、实时身份验证和 dirty 删除保护；同步精简测试与文档。

以下条目记录历史开发过程，不代表当前兼容承诺。

V0 是 Agent Steward 的独立设计合同与 `taskctl` 参考实现依据。它与 V1 以及未来可能出现的 V2 不构成必然的继承、替代或升级关系，各自范围和完成度单独判断。原始内容可通过 Git 历史查看；本目录允许为消除内部矛盾、安全歧义和不可实施合同而持续勘误。

## 2026-09-04

- 将 Task 主键升级为 `INTEGER PRIMARY KEY AUTOINCREMENT`；原字符串 ID 迁移为唯一、可空且只可设置一次的 `taskKey`；
- 将 Session、Checkpoint、Note、History 的 Task 外键及 JSON `taskId` 改为整数；CLI 同时接受 `12`、`#12` 和 `taskKey` 引用，迁移旧数字 Key 可用 `key:` 显式消歧，人类输出显示 `#id`；
- 增加无参数最小创建、完整 JSON 创建及受限 JSON Merge Patch；Task 描述允许暂缺，但设置后不能清空，且 `completed` 关闭前必须完整；
- `--json --input -` 支持从 stdin 安全读取 UTF-8 JSON，并稳定拒绝空输入、非法 UTF-8/JSON 和未知字段；文件输入保持可用；
- JSON envelope 升级到 `schemaVersion: 2`；
- Task list 增加 status/taskKey/title-goal-scope 文本筛选、绑定筛选摘要且长度固定的游标分页、字段投影，以及面向终端的 table/lines 输出；
- 增加原子 schema v7 migration；迁移在 SQLite writer transaction 中持有全部 v6 Task 旧身份锁，检测到仍活跃的 v6 Worktree operation 时拒绝升级；旧 Task 按 `created_at ASC, 原 id ASC` 分配数字 ID，并在同一事务中重建所有关系；旧 History payload 保持原文，迁移前 `task.created` 允许缺少新字段；补充回滚、ID 不复用、CAS、History、Session、Worktree、引用解析和 stdin 合同测试；
- 新写入的非空 Task title 统一校验 `MMDD｜类型｜主题`，类型限制为八类；增加 CAS `task retitle`，允许只修正 closed Task 的 title，不改变关闭状态或其他字段，并写入 `task.retitled` History。

## 2026-09-02

- 增加 Rust Cargo workspace 与 `taskctl` V0 可运行参考实现；
- 实现 Task、Session、Checkpoint、Note、History 和 Session Import 的 SQLite 闭环；
- 实现本地 Worktree create/status/remove/adopt/detach、安全观察和显式恢复；
- 增加稳定 JSON envelope、compare-and-swap、数据库权限告警和 `doctor`；
- 使用临时数据库与临时 Git 仓库增加单元、集成和 CLI 合同测试；
- 保持 V0 实现边界，不因其它版本方案中的 Daemon、GUI、MCP 或领域模型规划而扩大 V0 范围。

## 2026-09-01

- 冻结 Task Create/Patch、状态转换、expected version 和稳定 JSON DTO；
- 补齐 Session attach/resume/close、同 Task 复合外键和继续关系不变量；
- 增加 Worktree adopt/detach 以及 Git/SQLite 部分完成恢复合同；
- 定义跨平台 CanonicalPath、RepositoryIdentity 和 TOCTOU 复核规则；
- 将 Session Import 明确为 add/list/remove，增加 SHA-256 去重、逻辑删除和物理擦除边界；
- 冻结 History change type、最低 payload、错误码和警告码；
- 明确 claim/resume 只创建全新目标 Session，禁止 self-reference 和历史 Session 重激活，并固定 Session 输出排序；
- 禁止 `worktree create` 覆盖既有引用，以按 Task advisory lock 串行化外部操作，并在 `BEGIN IMMEDIATE` 写事务内实时扫描 Owner，阻止多个 Task 登记同一路径；
- 要求 Git 任意退出状态后重观测现场，增加 `GIT_COMMAND_FAILED`，并将无法排除外部变化的结果归入 `PARTIAL_EXTERNAL_STATE`；
- 增加本地数据库权限以及未来 Daemon 的认证、Origin 和 CSRF 边界。
