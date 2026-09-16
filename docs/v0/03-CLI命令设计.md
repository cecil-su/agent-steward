# CLI 命令设计

当前源码：SQLite Schema8、CLI/HTTP JSON envelope3、UI合同5、包格式1。普通打开只初始化空库或打开当前格式，不隐式升级。源码合同不证明已部署。项目、规则和迁移的专门合同见 [文档导航](README.md)。

## 通用约定

```text
taskctl [--database PATH] [--json] [--input FILE|-] [--yes] [--verbose] DOMAIN ACTION ...
```

- 机器调用使用 --json，不解析终端表格。stdout 单个 UTF-8 JSON 对象加换行，诊断仅 stderr；帮助/版本/解析错误也使用 envelope，不打开数据库。
- JSON 模式不交互询问；需要确认的操作显式 --yes。--verbose 不输出正文、凭据或参数值。-- 后字面值和选项值里的 --json 不启用 JSON 模式。
- --input 接受 UTF-8 JSON 普通文件或 stdin，通用上限16 MiB；拒绝设备/目录/FIFO，打开后复核类型。超限读取限额+1字节即拒绝，不等待超限流EOF；限额内 stdin 仍需结束。Rule/Profile 分别使用128 KiB/512 KiB专用上限。未知字段、空输入、非法JSON/UTF-8返回 INVALID_INPUT，不回显正文。
- 默认库是操作系统用户应用目录下 agent-steward/steward.db；开发/测试每次显式指定隔离库。Windows为 `%LOCALAPPDATA%/agent-steward/steward.db`。
- Task 引用为正整数、`#数字` 或可选 taskKey；Project 为正整数、`##数字` 或唯一名称。JSON ID 为整数；Shell中含 # 的引用加引号。taskKey 按原文匹配，不能纯数字或以 # 开头，只能从 null 设置一次。

## Task 命令

```text
taskctl task create [TASK_KEY] [--project PROJECT] [--input FILE|-]
taskctl task show TASK
taskctl task list [--status STATUS|active | --view VIEW] [--project PROJECT] [--task-key KEY] [--query TEXT] [--page-size 1..200] [--cursor CURSOR] [--fields FIELD,...] [--format table|lines]
taskctl task context TASK [--require-read-only] [--format markdown]
taskctl task status TASK STATUS --if-version VERSION
taskctl task update TASK --if-version VERSION --input FILE|- --yes --reason REASON
taskctl task retitle TASK --if-version VERSION --title TITLE
taskctl task project TASK (--project PROJECT | --clear) --if-version VERSION --yes --reason REASON
taskctl task components TASK (--component NAME ... | --clear) --if-version VERSION --yes --reason REASON
taskctl task note TASK --if-version VERSION --type decision|progress|risk --text TEXT
taskctl task notes TASK
taskctl task claim TASK --session SESSION --if-version VERSION [--take-over]
taskctl task resume TASK --session NEW_SESSION --if-version VERSION [--from-session OLD_SESSION] [--take-over]
taskctl task checkpoint TASK --session SESSION --if-version VERSION --input FILE|-
```

创建默认 todo/version1，描述、taskKey、项目可暂缺。title 非空时为 `MMDD｜类型｜主题`：有效月日、全角分隔符，类型为功能/设计/修复/优化/发布/探索/文档/研究；调用方按 Session 时间转换到 Asia/Shanghai 生成日期。描述非空字符串设置后不能用 null 清除；nextStep 可清空。创建示例：

```json
{"title":"0916｜修复｜合成示例","goal":"目标","scope":"范围","acceptanceCriteria":"验收标准","nextStep":"下一步"}
```

status 为 backlog/todo/in_progress/in_review/blocked/done/cancelled，任意来源可直接转换。仅修改 status/version/updatedAt 及 task.status_changed History；CAS 后同值 no-op。没有业务流程前置条件，不结束/创建 Session、不清空记录。旧 task block/unblock/pending-release/continue/close 已删除。详见 [七状态](21-待上线任务状态.md)。

update 只允许 taskKey/title/goal/scope/acceptanceCriteria/nextStep/project/components；必须 --yes 和非空 reason。禁止修改状态、Session、Checkpoint 或历史关闭/阻塞字段。空/无变化 Patch 拒绝，多字段只增一次版本。全部状态均可维护 nextStep、追加 Note、修正标题。项目/组件关联的原子维护见 [信息维护](20-任务信息维护.md)。

Note 类型与正文校验、CAS、History 保持；无需先领取，sessionId 为当前关联或 null，不隐式恢复历史会话。正文按原文存储，可安全 Markdown 展示，Web 无写入口。

claim/resume 只管理 Session；Checkpoint 只要求同任务未结束的当前 Session，不限制业务状态。新 Checkpoint gitHead=null；旧值保持历史含义。完整身份、接管和导入合同见 [Session](09-AI-Session记录.md)。

## 查询、分页与上下文

- list 默认50条，1–200；updatedAt DESC、id ASC，返回 nextCursor/hasMore/pageSize。游标绑定 status/taskKey/query/项目ID，条件变化不得复用，不保证跨请求静态快照。
- view 为 active/backlog/todo/in-progress/in-review/blocked/done/cancelled/recent；active 排除 done/cancelled，recent 不限状态；view/status 互斥。
- query 为数字或 #数字时精确匹配ID，前导零等价、越界拒绝；其它文本对 title/goal/scope 做转义 LIKE，%/_ 为普通字符。所有筛选取交集。
- fields 为 TaskView 的20个字段白名单；未知/重复/空字段拒绝。终端 table 可截断展示，JSON不截断字段；lines 要求一个字段且不能与 --json 组合。
- context 使用专用 READ_ONLY|NOFOLLOW/query_only 连接，不建库、不修改权限或 journal_mode。--require-read-only 是兼容断言，旧客户端拒绝时不得去掉选项重试。SQLite读取WAL可能涉及SHM，不承诺零文件系统活动。
- 同一读事务返回 task/checkpoint/session/project/projectProfile/sessionRules/notesSinceCheckpoint/notesTruncated。只带检查点之后最新50条Note，按History sequence确定边界，超限明确警告；完整Note用 task notes。规则完整返回，失败不伪装成空规则。
- context/resume 不含 worktreeStatus、不读 Git 或源码。task here、project here/context/source resolve 和 worktree 命令已移除。

## Session、Hook与诊断

```text
taskctl session list [--task TASK]
taskctl session show SESSION
taskctl session attach TASK --session SESSION --if-version VERSION [--source CLIENT] [--external-session ID] [--record-path PATH]
taskctl session bind SESSION --source CLIENT --external-session ID --if-version VERSION
taskctl session close SESSION --if-version VERSION
taskctl session import add TASK --session SESSION --if-version VERSION --file FILE --confirm-sensitive-content-reviewed
taskctl session import list SESSION
taskctl session import remove IMPORT --if-version VERSION --yes
taskctl --input EVENT_JSON hook ingest
taskctl hook list SESSION --after 0 --limit 100
taskctl hook clear SESSION --if-version VERSION --yes
taskctl history TASK
taskctl doctor
```

所有面向已有Task的业务写操作检查调用方的最新version。Hook ingest按事件键去重，不改变Task version/History；Hook clear使用Task CAS并保留去重墓碑。doctor检查Schema、quick_check、外键和Session记录路径引用，不查Git、不修复外部现场。

## JSON envelope与DTO

```json
{"schemaVersion":3,"ok":true,"data":{},"warnings":[],"error":null}
```

失败为 ok=false/data=null，error 含 code/message/retryable/details；输出可空字段显式 null，消费者忽略未来新增字段；输入未知字段拒绝。破坏兼容性时升级版本，不能只靠旧 envelope 接受新状态。

| DTO | 字段 |
| --- | --- |
| TaskView | id/projectId/componentIds/taskKey/title/status/version/goal/scope/acceptanceCriteria/nextStep/blockReason/blockRecovery/currentSessionId/latestCheckpointId/closureOutcome/closureReason/closedAt/createdAt/updatedAt |
| SessionView | id/taskId/source/externalSessionId/continuedFrom/recordPath/startedAt/endedAt |
| CheckpointView | id/taskId/sessionId/summary/completed/decisions/pending/nextStep/risks/gitHead/createdAt |
| TaskNoteView | id/taskId/sessionId/noteType/text/createdAt |
| SessionImportView | id/sessionId/sourcePath/mediaType/sha256/sizeBytes/importedAt，不含BLOB |
| HistoryEntry | id/taskId/sequence/changeType/sessionId/occurredAt/summary/payload |

Task 无Repository/Worktree字段。旧closure/block字段只是历史事实，与当前status无约束，不由status新生成或清除。时间为UTC RFC3339，Session/Checkpoint/Import ID与hash是字符串，Task和自增记录ID为整数。

成功 mutation 返回 data.task 和提交版本；note/checkpoint/Session命令分别附 note/checkpoint/session；resume 附 checkpoint/sessions/sessionRules/nextStep。History按sequence升序，Session按startedAt/id升序。响应必须是本次事务快照；数据损坏不可用时不得用空数组掩盖。

| 错误 | exit | 说明 |
| --- | ---: | --- |
| INVALID_INPUT / NOT_FOUND / UNSUPPORTED_SCHEMA_VERSION | 2 | 输入、引用或数据库格式错误 |
| VERSION_CONFLICT | 4 | expectedVersion/currentVersion；须重读并判断 |
| SESSION_CONFLICT / CONSTRAINT_VIOLATION | 4 | 身份冲突或约束拒绝 |
| DATABASE_BUSY | 4 | 有限等待后繁忙，不自动覆盖 |
| PATH_IDENTITY_UNKNOWN / FILESYSTEM_UNAVAILABLE | 5 | 需要实际文件操作的路径/文件系统失败 |
| DATABASE_UNAVAILABLE | 10 | 数据库/存储数据不可用 |

retryable 不等于授权自动重放。响应丢失先读取核对；不提供请求级 exactly-once。权限、逻辑删除与敏感内容警告见 [安全](05-安全与事务.md)，Hook错误与HTTP映射见 [HTTP](11-Hook与HTTP合同.md)。

## 数据库离线复制

`database import-schema2/4/5/6/7` 按命令数字匹配冻结输入格式，目标为Schema8；`import-v7`为独立schema_migrations归档格式。必须明确源/新目标、--yes，禁止原地修改/覆盖/默认库回退。schema4/5/6/7均支持--source-paths JSON文件，为旧Git源码显式提供sourceId→绝对路径映射后直接导入8；缺映射拒绝，不读取Git或猜测路径，也不要求先升级到7。准确支持范围、安全约束及命令见 [隔离导入](18-Schema7隔离导入与验证.md)。
