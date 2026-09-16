# Schema8 隔离导入与验证

## 格式与入口

普通连接仅初始化空数据库或打开Schema8，不原地转换。以下命令数字表示**源格式**，目标均为Schema8；不能仅修改user_version冒充兼容。

| 命令 | 输入格式 | 范围 |
| --- | --- | --- |
| import-schema2 | user_version=2，冻结schema2.sql布局 | Task/Session/Checkpoint/Note/History/Import/Hook；项目、资料、规则为空 |
| import-schema4 | user_version=4，冻结schema4.sql布局 | 包含项目/组件/源码关系；资料和规则为空 |
| import-schema5 | user_version=5，冻结schema5.sql布局 | 包含项目资料；规则为空 |
| import-schema6 | user_version=6，冻结schema6.sql布局 | 包含pending_release；规则为空 |
| import-schema7 | user_version=7，冻结schema7.sql布局 | 包含规则/规则历史；Git来源需显式路径映射 |
| import-v7 | user_version=0，schema_migrations版本1–7的独立归档布局 | 仅闭合且无当前Session/Worktree引用的归档；见 [归档合同](13-v7归档迁移.md) |

没有import-schema3。archive-v7不等于user_version=7。输入定义位于 [migration目录](../../crates/application/src/migration)，普通建库Schema在 [storage-sqlite](../../crates/storage-sqlite/src/lib.rs)。

## 显式转换

| 源任务状态 | 目标状态 |
| --- | --- |
| open | todo |
| in_progress | in_progress |
| pending_release | in_review |
| blocked | blocked |
| closed + completed | done |
| closed + partial/cancelled/superseded | cancelled |

保留closure_outcome/closure_reason/closed_at及block_reason/block_recovery为历史事实，不再约束当前状态。保留Task ID/version、创建/更新时间、Session身份/continuedFrom/endedAt及当前关联、Checkpoint和历史git_head、Note、History的ID/sequence/changeType/payload原文、Import BLOB与Hook墓碑。迁移不增加业务History、不恢复或结束Session、不按文本推断状态/项目/规则。

删除当前Task的repository_path/repository_common_dir/repository_branch/worktree_path和repositories表，旧History中这些字段仍原样保留。源码资料目标仅id/project_id/component_id/directory_path/created_at：旧directory来源保留路径；旧Git来源不能从common-dir/相对路径推断工作区，必须显式给出sourceId→绝对路径。

```json
{"12":"E:/recorded/project","27":"/recorded/other-source"}
```

`import-schema4/5/6/7 --source-paths paths.json`均使用上述映射；无Git来源可省略。拒绝遗漏Git来源、重复/未知ID、为directory来源多给映射、非法路径/JSON及尾随内容，不访问映射路径、不检查Git或目录存在性。

schema2无源码表，不需要映射。schema4/5/6/7有Git来源时均可携带显式映射直接导入Schema8，无需先升级到7；缺映射拒绝。Service的旧无映射wrappers默认使用{}，因此不能隐式处理含Git来源的快照。不得修改源版本号后套用其它入口。

## 文件与数据安全

- 源/目标都须显式绝对路径、明确文件名，--yes；不回退默认库，不原地修改、合并、覆盖、切换配置或启动/停止服务。
- 源必须是可信本地普通数据库快照，布局/表列/索引/触发器/约束/版本与冻结输入一致。拒绝漂移布局、歧义taskKey、无效JSON/BLOB/序列、损坏完整性/外键；源与目标父目录由操作者控制。
- 先停止所有源写入者，使用SQLite Backup API取得包含已提交WAL的一致快照；不能只复制运行中主文件。固定读事务、data_version和身份复查不等于停写屏障，不能排除检查后的替换。
- 源及已有sidecar须普通文件，不是链接/目录。迁移文件路径拒绝网络/设备前缀、ADS、父级遍历、末尾分隔符和Windows分量尾随句点/空格。
- 目标及-wal/-shm/-journal须全部不存在，包括悬空链接；检查前后都不删除占用路径。目标父目录私有，已存在目录只验证，不chmod/重写ACL；缺失时仅创建最后一级私有目录，祖先须存在。
- 只读源，固定快照；同目录私有暂存库初始化Schema8，以单IMMEDIATE事务、延迟外键、明确列复制和转换。只读SQLite访问WAL可能涉及SHM，不承诺sidecar元数据逐字节不变。
- status及退役字段/源码路径使用显式转换投影核验；其他保留字段逐字段、typed hash、JSON原文/BLOB核验，不因有转换而跳过历史一致性检查。保留保留表的自增高水位，不能让已删除记录ID重新分配。
- 不读取登记源码、Worktree或Import外部原始文件，不调用Git。文件/路径资料复制不证明外部现场或赋予Session执行权。
- Schema7规则当前视图及全部规则历史均须通过应用解码；历史before/after快照须满足身份、版本和内容格式校验。非法历史拒绝发布，不改写JSON原文。
- 完成完整性、外键与应用解码后，关闭并刷盘暂存库，复核源/父目录身份及目标占用，以不覆盖hard link发布；不支持hard link即失败，不降级覆盖。
- 普通失败仅清理本次暂存，不发布部分目标。进程中断可能留下暂存，重试不扫描/采纳/删除它，不自动清理用户目录。

## 输出与核验

schema2/4/5/6/7入口返回sourceSchema/targetSchema、counts、highWaterMarks、tableSha256、verified及转换遗漏字段说明。targetSchema=8；`digestScope=retained-fields-after-explicit-schema8-conversion`，不声称转换前后原始整表hash相等。

`digestEncoding=sqlite-typed-rows-v1`按保留投影和主键排序：行前缀R，NULL为N，INTEGER为I+i64小端，REAL为F+IEEE754小端，TEXT/BLOB为T/B+u64字节长度小端+原字节，不重序列化JSON。

`verified=true`仅说明实现的复制检查满足；`sourceOpenedReadOnly=true`、`sourceQuiescenceVerified=false`、`externalPathsObserved=false`明确未证明停写或外部现场。摘要不证明语义真实或跨进程身份连续性。单Import上限16MiB，没有整库内存/总耗时硬保证。

## 隔离操作

CLI/SOURCE/NEW_DB必须替换为核验的候选程序、合成或获准停写快照、不存在的隔离目标；不得直接对正式库套用。

```text
CLI --json database --help
CLI --json database import-schema7 --help
CLI --database NEW_DB --json --yes database import-schema7 --source SOURCE --source-paths paths.json
```

1. 核对源格式、候选程序、映射与目录权限，确认全部源写入者已停。
2. 确认目标/sidecar不存在，保留源快照；按格式选入口，不改版本号规避检查。
3. 检查Schema8、计数/保留字段/摘要/高水位/完整性及会话不变，核对新旧关闭含义。
4. 显式指定新库读取Task/Session/History/Note/Import元数据及context。doctor还会检查Session记录路径存在性，须在这些路径也获准时运行。
5. 隔离故障测试覆盖错误Schema/布局、损坏、映射缺项、目标占用、sidecar、非私有父目录、写入/发布中断；失败不出现部分目标。

验证入口：`cargo test -p steward-application migration --locked`、application的migration集成测试、CLI schema2_contract/migration_contract、server schema2_startup/database_preflight。测试文件存在不等于已运行。

## 正式切换与未验证边界

正式切换独立确定停写窗口，暂停CLI/Hook/taskd/宿主所有写入者，一致备份、逐字段核验、数据库路径和配套程序/UI合同核对后成套切换；导入命令不停止已有连接，也不阻止旧副本继续写入。

新库尚未写入时可按停写恢复方案恢复备份及配套程序；接受新写入后不得直接覆盖备份或只降级二进制，必须保留两侧数据并明确对账。无自动降级/反向合并保证。

正式数据、生产规模、跨平台、磁盘满/断电、恶意同用户路径替换、安装切换/恢复及人工业务验收须分别验证。Schema8源码、文档或隔离复制成功不代表正式部署完成。
