# V0 修订记录

## 2026-09-08：并发首次启动的 WAL 竞争修复

- 首次开启 WAL 遇到 SQLite 锁升级直接返回 BUSY 时，在五秒期限内释放语句后重新检查模式并重试；已有 WAL 数据库继续只读检查，不重放业务写入。
- 增加持有写事务时开启 WAL 的确定性回归，以及八轮、每轮十二连接同时首次打开新数据库的回归。

## 2026-09-08：Linux 路径身份与 CI 回归修复

- Linux 使用随身份快照保留的 `O_PATH` 句柄固定文件对象，避免删除后重建目录复用 inode 而漏报替换；对象身份取自同一句柄，不读取文件内容。
- 跨平台目录替换测试保留旧目录后再创建替代目录；Linux 另行覆盖删除重建、快照克隆、元数据专用句柄和 close-on-exec，保留原来的拒绝替换断言。
- 增加同一祖先内普通文件变化不误报身份替换的回归测试。

## 2026-09-08：V0 文档一致性 review

- 以 `17eaf3e` 为源码对照，统一 M1–M3 历史范围与当前 M4/M5 实现表述。
- 修正早期每次启动新凭据、仅内存认证、仅回环和无 SSE 的过时合同；当前角色、本机直连与严格认证统一引用 11/12。
- 补齐 session_events、浏览器授权辅助存储、专用 v7 归档导入边界、Hook/notes 命令及独立 UI 发布入口。
- 将首轮 77 项测试明确绑定 d43e0a9；删除当前未推送等易过期断言，不把 CI 配置当作通过证据。
- 修正 closed Task 描述更新例外、原生独立投递去重边界及 synthetic Task 显式关闭步骤；根 README 的 Workspace/Review 原则标明仅属 V1。
- 仅修订文档与验收要求；未重跑运行时测试，也未改变产品权限、Schema 或状态机。

## 2026-09-08：Xuanwu 参考整理

- 新增第 15 篇，重新基于 V0 实现整理可借鉴点与不采用项，不沿用 V1 模型。
- 补充 Checkpoint 记录、适配器验收分层、故障恢复和真实接续旅程建议。
- 更新索引、路线、测试和使用文档入口；仅文档变更，未执行新增旅程或改变功能状态。

## M4：Codex 与 pi 原生适配

- 增加 Codex 原生 stdin 投影、被动 JSON 响应与 hooks.json 配置生成器。
- 增加 pi 扩展：生命周期、消息类型、工具及空闲观察，显式会话匹配与故障回退。
- 4 项原生契约集成测试通过，77 项 Rust 回归通过；未执行真实客户端模型运行。
- 明确原生时间为适配器观察时间，独立重复投递不保证去重。

## M4/M5：通用 Hook 与本地工作台

- 修复外部 Session 绑定与生命周期语义，增加 CAS 一次性 `session bind`；
- 增加独立 `session_events`、有界输入、幂等去重、分页、容量与清除防复活；观测不改 Task version；
- 增加只投影元数据的 `task-hook` 通用宿主适配器，不采集消息/工具正文；
- 增加 `taskd` 同源 HTTP/GUI、启动凭据、严格 Host/Origin/CSRF 边界，共用权限告警；
- GUI 支持任务编辑、版本冲突保留输入、Session 交接、Checkpoint、Import、History 和显式安全 Worktree 操作；
- 数据库 schema 提升到 2，继续拒绝旧库且不迁移；新增 Rust 与真实浏览器验收。


## 日常查找与交接

- 增加 `task here` 只读目录定位和 `task context` Markdown/JSON 上下文导出；
- 列表增加固定视图和默认下一步列；resume 终端输出改为分段摘要；
- 不新增持久化模型，不改变领取、恢复、CAS 和关闭规则。


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
