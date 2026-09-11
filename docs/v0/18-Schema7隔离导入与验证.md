# Schema7 隔离导入与验证

## 接口与输入格式

普通连接只初始化空数据库或打开Schema7，不进行原地转换。`taskctl database --help`提供以下显式离线复制命令；目标均为当前Schema7，命令名中的数字指定源格式，不能改为7或互换使用。

| 命令 | 必须匹配的源 | 复制范围与目标空表 |
| --- | --- | --- |
| `import-schema2` | `user_version=2`及冻结schema2.sql完整布局 | 七个业务表、四类自增高水位；任务project_id为NULL，项目/资料/规则关系为空 |
| `import-schema4` | `user_version=4`及冻结schema4.sql完整布局 | 13个业务表、九类自增高水位；项目资料和规则表为空 |
| `import-schema5` | `user_version=5`及冻结schema5.sql完整布局 | 14个业务表、九类自增高水位，包含项目资料；规则表为空 |
| `import-schema6` | `user_version=6`及冻结schema6.sql完整布局 | 14个业务表、九类自增高水位，包含待上线状态；规则表为空 |
| `import-v7` | `user_version=0`、schema_migrations记录版本1–7的归档布局 | 仅全部关闭且无当前Session/Worktree引用的Task归档；独立限制见[v7归档导入](13-v7归档迁移.md) |

没有`import-schema3`或`import-schema7`命令。Schema7与`schema_migrations`格式v7不是同一种输入；单改`user_version`不能满足完整布局检查。

实现与输入定义：[`schema2.rs`](../../crates/application/src/migration/schema2.rs)、[`schema2.sql`](../../crates/application/src/migration/schema2.sql)、[`schema4.sql`](../../crates/application/src/migration/schema4.sql)、[`schema5.sql`](../../crates/application/src/migration/schema5.sql)、[`schema6.sql`](../../crates/application/src/migration/schema6.sql)。

## 数据与文件安全合同

本节适用于`import-schema2/4/5/6`的共享复制实现；`import-v7`按其独立归档合同执行。

- 每次导入必须显式提供`--database`、`--source`和`--yes`，不回退默认数据库。只读源，禁止原地修改、合并或覆盖；命令不安装程序、不切换配置、不启动或停止服务。
- 源必须是可信本地普通数据库快照，布局、索引、触发器、约束、应用字段和源版本都须匹配对应输入定义；拒绝漂移布局、歧义引用、无效JSON/BLOB/序列、完整性或外键损坏。
- 使用本地绝对路径和明确文件名；拒绝网络/设备前缀、ADS、父级遍历、末尾分隔符及Windows分量末尾句点/空格。源主文件及存在的sidecar必须是普通文件，不能是链接或目录。
- 目标主文件及`-wal/-shm/-journal`必须全部不存在，包括悬空链接。目标父目录必须私有且由操作者控制；存在的目录只验证权限，不chmod或重写ACL。缺失时只创建最后一级私有目录，祖先须存在。
- 操作者必须停止全部源写入者，再使用SQLite Backup API取得包含已提交WAL的一致快照；不得只复制运行中数据库的主文件。固定读事务及`data_version`复查不能代替停写，不能阻止检查后的并发替换。
- Schema2/4/5/6复制在同目录私有暂存库中初始化Schema7，以单个IMMEDIATE事务和延迟外键按明确列复制；逐字段核验JSON原文、BLOB、ID、版本、状态、时间戳、Session继续关系、History、Hook删除墓碑及自增高水位。不根据任务文字推断项目、规则或任务状态。
- 核验不读取登记的Worktree、源码根或Import原始文件，不调用Git；复制数据库引用不证明外部路径仍存在，也不授予Session执行权。
- 完成完整性、外键及应用解码校验后，关闭并刷盘暂存数据库，复核源/父目录身份与目标占用，用不覆盖的hard link发布。文件系统不支持hard link即失败，不降级覆盖。
- 普通失败只清理本次暂存；进程中断可能残留私有暂存，重试不扫描、采纳或删除它。已有目标及sidecar不自动清理。

## 输出合同

Schema2/4/5/6复制返回源/目标路径及`sourceSchema/targetSchema`、`counts`、`highWaterMarks`、逐表`tableSha256`和`verified`。

- `targetSchema=7`。
- `verified=true`仅表示该次复制校验满足实现检查，不表示业务验收或正式切换完成。
- `sourceOpenedReadOnly=true`；SQLite读取WAL时可能涉及SHM，不承诺sidecar元数据逐字节不变。
- `sourceQuiescenceVerified=false`、`externalPathsObserved=false`明确未证明停写和外部现场。
- `digestEncoding=sqlite-typed-rows-v1`：按源列顺序和主键排序，行前缀`R`，NULL为`N`，INTEGER为`I`+i64小端，REAL为`F`+IEEE754位小端，TEXT/BLOB为`T/B`+u64字节长度小端+原字节；不重新序列化JSON。

摘要仅用于复制核对，不是语义真伪、身份连续性或缓存授权。单个Import最多16 MiB；没有整库内存或总耗时硬上界。

## 隔离执行步骤

以下占位符必须替换为获准的本地绝对路径，不能使用默认库或正式运行数据库。SOURCE只能来自已准备的合成/获准停写快照。

```text
taskctl --json database --help
taskctl --json database import-schema6 --help

taskctl --database NEW_ABSOLUTE_DB --json --yes database import-schema6 --source QUIESCENT_SCHEMA6_DB
```

1. 核对二进制身份、对应命令帮助、源输入格式、源目录权限及全部写入者停写状态。
2. 确认目标私有目录与目标/sidecar无占用；保留原快照，不清空现有目标来规避检查。
3. 按表格选择匹配输入的命令，仅运行一次；失败后保留诊断并调查，不盲目重放。
4. 检查返回目标Schema7、复制计数、逐表摘要、自增高水位与`verified`，核对源业务内容未变化。
5. 使用同一候选CLI显式指定新库读取`task list --view recent`、Task/Session/History/Import元数据。`task context`和`doctor`可能观察登记的外部路径，只能在这些路径也获准且隔离时执行。
6. 对输入超限、错误Schema/布局、源损坏、目标已存在、sidecar占用、非私有父目录以及复制/发布中断运行隔离回归，确认拒绝时不发布部分目标。

可执行的验证入口：

```bash
cargo test -p steward-application migration --locked
cargo test -p taskctl --test schema2_contract --test migration_contract --locked
cargo test -p steward-server --test schema2_startup --test database_preflight --locked
```

这些命令使用测试夹具，不是正式迁移命令。测试脚本存在不代表目标平台或正式数据已验收。

## 切换、恢复及未验证边界

正式切换须独立确定维护窗口，暂停CLI、Hook、taskd及宿主适配器等全部写入者；一致备份、逐字段核对、程序/UI合同与数据库路径核对完成后，才可成套切换。导入命令和版本拒绝不能停止已有连接，也不能阻止另一份库继续接收写入。

Windows启动器只接受Schema7，不执行普通跨Schema更新；安装/停启规则见[Windows指南](../../distribution/windows/README.md)。恢复写入前可在停写状态依照核验方案恢复备份及配套程序；新库接受写入后不得直接用备份覆盖或只降级二进制，必须停写、保留两侧数据并明确对账/恢复方案。没有反向合并或自动降级合同。

生产规模、恶意同用户路径替换、磁盘满、断电耐久性、跨平台专项、实际安装切换及恢复后的业务完整性必须分别验证。复制、自动化测试或页面可访问不构成这些验证的替代。
