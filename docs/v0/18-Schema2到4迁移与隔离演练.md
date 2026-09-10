# Schema 2 → 4：迁移方案与隔离演练

> #34，2026-09-08。**迁移入口已提交推送至 `e4c5436` 且对应 CI 通过；Schema 4 安装/启动兼容正在开发工作区隔离验证。尚未发布、安装或在正式库执行迁移。**
> Git 基线 `0d34375` 已推送；本轮开发未读取、复制或迁移正式业务数据库，未全局安装二进制、切换服务或发布。
> 正式 Task 连续性仍通过已安装的 taskctl 管理；演练脚本不访问该数据库。

## 1. 选择：显式复制到新库，不原地 ALTER

沿用 [v7 归档迁移](13-v7归档迁移.md) 的“只读源快照 → 当前 Schema 私有暂存库 → 核验 → 不覆盖发布”原则，但**不能调用 `database import-v7`**：它只接受旧 `schema_migrations` v7 闭合任务归档，且旧库没有 `session_events`。

Schema 2 专用入口为 `Service::import_schema2` / `database import-schema2`，支持含未关闭任务的停写快照。普通 `open_database` 继续拒绝不支持的版本，没有隐式升级，也不能只修改 `PRAGMA user_version`。

选择逻辑复制而非改造原表：目标由当前 Schema 4 初始化器产生，直接取得完整约束、触发器、列布局及索引；避免重建含 Task/Session/Checkpoint 循环外键的旧表、覆盖源库或依赖手写增量 DDL。复制不会安装程序、切换默认路径或修改任何 Task 的业务状态。

### 必须保留的数据

| 对象 | 保留要求 |
| --- | --- |
| `tasks` | ID、key、描述、状态、version、block/closure 信息、Session/Checkpoint 引用、四个 Repository/Worktree 字段、全部时间戳原样保留；支持 open/in_progress/blocked/closed |
| `sessions` | ID、Task、source/external identity、continued_from、record_path、开始/结束时间；不自动结束、claim、resume 或接管 |
| `checkpoints`、`task_notes`、`history` | 所有列，含 JSON 原文、顺序、Git HEAD；不伪造迁移 History，不递增 Task version |
| `session_imports` | 元数据、SHA-256、原始 BLOB 字节；不重新读取 record_path/source_path 指向的会话正文 |
| `session_events` | sequence、event_id、fingerprint、可见观测及 kind/时间均为 NULL 的删除墓碑；重试不能复活已清理事件 |
| `sqlite_sequence` | tasks/task_notes/history/session_events 四个 AUTOINCREMENT 高水位；不能仅根据现存行的 MAX 重建 |
| 新项目关系 | `tasks.project_id=NULL`；projects/project_history/components/repositories/source_roots/task_components 均为空，不从文字或 Worktree 推断项目 |

数据库复制不证明磁盘源码身份连续性，也不授予旧 Session 新执行权限；Worktree 必须重新实时观测。Host/Pi 证据仍不允许复用。

## 2. 已实现的显式复制合同

1. 独立命令要求显式确认、绝对源/目标路径，禁止缺省正式库路径、原地修改、覆盖及合并。规范化并核验路径身份；源必须为受支持的普通本地数据库快照。目标及 `-wal/-shm/-journal`（包括悬空链接）须全部不存在，目标目录须独占且权限受控。
2. 支持范围不只检查 `user_version=2`：对白名单表、列、索引、触发器/约束结构做严格验证，拒绝未知或漂移布局、歧义引用、无效 JSON/BLOB/序列元数据、完整性或外键损坏。Schema 0/1/3/4 不能伪装成成功的 2→4 升级。
3. 源以 READ_ONLY/query_only 和固定读事务取得一致视图；需要备份时使用 SQLite Backup API，纳入已提交 WAL，不能复制运行中数据库的单个主文件。不把 `immutable=1` 用在仍可变化的源上。
4. 同目标目录创建私有暂存库，通过当前初始化器产生 Schema 4。在单个 IMMEDIATE 事务中启用 deferred foreign keys，按显式列流式复制七个业务表和合法序列高水位。
5. 同一快照内逐行逐字段比对所有旧列，保留四类序列；新关系断言为空，`integrity_check`（含索引/表一致性）、外键及应用解码校验通过。检查 Import BLOB/SHA、可见 Hook 的规范指纹及墓碑指纹形状。迁移仅解码数据库，不调用会探测持久化 Worktree 的 `task_context`；外部路径留给用户后续显式实时观测。
6. 提交后切换暂存库到 DELETE journal、关闭连接、刷盘；检查源文件/目标父目录身份、源 `data_version`、目标占用，再通过不覆盖的 hard link 发布。普通失败清理本次暂存目录；强杀可能残留私有暂存，重试不扫描/采纳/删除它，也不删已有目标/sidecar。文件系统不支持 hard link 时拒绝，不降级覆盖。
7. 外部证据记录工具提交及 SHA-256、源/目标 Schema、各表计数与内容摘要、高水位、校验结果。该报告不成为第二个任务状态库，也不写入源业务记录。

实现：[`schema2.rs`](../../crates/application/src/migration/schema2.rs)，独立于 v7 范围校验，复用其目标占用与完整性辅助检查。源布局与冻结的 [`schema2.sql`](../../crates/application/src/migration/schema2.sql) 对照，基于 `8e91fd0`，不从 Schema 4 倒推或按版本号猜测兼容。

```text
<新版taskctl绝对路径> --database <不存在的新库绝对路径> --json --yes database import-schema2 --source <停写快照绝对路径>
```

- 两种 database import 都在缺少显式 `--database` 时拒绝，不回退或检查默认数据库。源/目标须为本地绝对路径，不接受网络/设备前缀、ADS 或父级遍历；源主文件及已存在 sidecar 必须为普通文件，拒绝直接链接/目录。
- Schema 2 入口额外拒绝 Windows 任一路径组成部分末尾的句点/空格（含扩展路径），源/目标参数必须以明确文件名结束，不接受尾随分隔符或 `/.`。不通过静默 trim 改写目的地，避免原参数、SQLite 与字面量 hard link 指向不同文件。普通中文名、内部空格和规范扩展路径仍可用；此检查不改动 HTTP checkout 白名单。
- 源 sidecar 检查从已取得的文件身份之 `canonical_path` 派生，与随后传给 SQLite 的源路径一致，不再使用规范化前的输入拼写。仍需可信、停写的源目录，不宣称解决恶意并发替换。
- 目标父目录已存在时必须私有（Windows 受保护且只授权认可主体的 ACL；Unix 当前用户所有且 group/other 无权限），不会 chmod 既有目录。缺失时只新建最后一级私有目录，祖先需已存在；源和目标目录均须由操作者控制并在操作期独占，权限/身份复查不是恶意同用户进程沙箱。
- 一致读事务纳入已提交 WAL；源主文件不经当前初始化器打开，不执行写 SQL。WAL 读取可能涉及 SHM，不承诺 sidecar 元数据逐字节不变。`data_version` 检测到复制期间其他连接提交就拒绝发布，但检查后仍有竞态，**不能替代停止全部写入者**。
- 输出 counts、四类 highWaterMarks、逐表 tableSha256、源/目标路径和版本。`digestEncoding=sqlite-typed-rows-v1`：源列顺序、主键排序，每行 `R`，NULL=`N`，INTEGER=`I`+i64小端，REAL=`F`+IEEE754位小端，TEXT/BLOB=`T`/`B`+u64字节长度小端+原字节；不重新序列化 JSON。摘要为复制核对证据，不是内容语义/身份连续性或缓存授权。
- `verified=true` 表示该次复制校验通过；`sourceOpenedReadOnly=true`、`sourceQuiescenceVerified=false`、`externalPathsObserved=false` 明确保留边界。不输出原始业务正文或 Import 内容；不生成 Task mutation，不安装/切换服务。
- 数据按行复制和校验，单个 Import 限制沿用 16 MiB；没有宣称整库内存或总耗时硬上界。磁盘满/权限错误按失败返回，不自动重试业务复制。断电耐久性、恶意路径竞态及实际正式库仍未验。

## 3. 停写、切换及回滚（均待单独授权）

### 停写前

- 盘点实际 CLI/task-hook/taskd、后台服务/计划任务、Pi/Codex 适配器及数据库配置路径，协调实际相关任务/会话（不按旧编号推定发布负责人）；不依据数据库中的引用推测运行进程。
- 记录版本、二进制/UI 身份、运行参数、目录权限和可恢复方案；不将 token/cookie 写入报告。
- 先保存 #34 的 Checkpoint，再进入明确维护窗口。**此后包括 Agent Steward 工具、Hook 在内的全部写入者都要暂停**，不能一边保存任务进度一边制作“最终”迁移快照。

### 最终快照与切换

- 确认旧进程退出、无在途写入，停止所有自动重启来源；取停写后的最终一致备份，演练期间的副本不能替代它。
- 核验备份可由旧工具恢复；执行经过验证的新库复制并复核业务数据、序列及权限，保留原库与备份。
- 优先在受控停机窗口保持既有正式数据库路径，成套切换 CLI/Hook/taskd 与数据库。Schema 4 启动器普通更新拒绝已有 Schema 2 安装；使用新的程序安装目录，明确配置迁移后的库/原 runtime，保留旧程序目录但不自动降级。自定义安装入口和只读版本预检见 [Windows 指南](../../distribution/windows/README.md)。具体文件操作和服务命令必须在路径/ACL/sidecar/平台占用确认后制定，本文不给出可直接覆盖正式库的脚本。
- 老客户端新开 Schema 4 会被拒绝，**但这不能阻止仍持有旧库连接的进程，或继续指向另一个 Schema 2 文件的客户端写入**。不能以 schema guard 替代停写与路径核对，避免两个库分别接收新数据。
- 先只读检查 doctor、Task 列表/context、Session/History/Import/Hook 去重状态，再明确决定恢复写入。人工验收本轮虽已跳过，正式切换权限及恢复校验并未自动获准。

### 回滚边界

- 恢复业务写入前：保持停写，用已验证的旧工具和停写备份按明确方案恢复，重新核验配置、权限及无残留 sidecar 后才恢复服务。
- 新库已接受写入后：不能直接覆盖回 Schema 2 或只降级二进制。立即停写并保留两侧证据，由用户决定修复前进或显式数据对账；项目关系没有已实现的降级映射。
- 回滚不是重放 History 或无条件重试旧 CAS，不自动关闭/重开 Task，也不进行 Git reset/stash。

## 4. 可重复的合成演练

脚本：[crates/cli/tests/schema2_rehearsal.py](../../crates/cli/tests/schema2_rehearsal.py)。只接受两个可信 CLI 可执行文件参数，**不接受源/目标数据库路径**；所有数据库、导入 BLOB 和临时 Git 仓库在自建 TemporaryDirectory 中产生，结束后删除，不启动服务。

旧 CLI 来自 `8e91fd08a0dd9d424b10de24200d6b669edce011` 的 `git archive`，解包到忽略目录后以独立 Cargo target 离线构建。没有切换工作树、安装工具或调用正式库的全局 CLI。当前脚本已移除 Python 复制内核，调用新 Rust CLI 的 `database import-schema2`（包括拒绝及发布路径），再独立逐字段核对。新 CLI 为 `0d34375` 基线上的本轮未提交构建。

本机复跑（已存在的隔离构建路径，不是全局安装）：

```bash
python crates/cli/tests/schema2_rehearsal.py \
  E:/ai/agent-steward/.local/task34-schema2-target/debug/taskctl.exe \
  E:/ai/agent-steward/.local/task34-target/debug/taskctl.exe
```

其他环境需自行构建上述可信旧/新 CLI 后传入其绝对路径；脚本不下载依赖、不自动构建或在找不到二进制时回退全局工具。依赖 Python 3.12+、Git、两版 CLI；未加入默认 Cargo/CI 测试。

### 初版 Python 原型 Windows 结果（历史）：10 组检查通过

- 旧 CLI 创建 open/in_progress/blocked/closed 四任务、四 Session（含继续关系和已结束 Session）、一 Checkpoint、一 Note、16 History、一含 `NUL/0xff` 的 Import、两条 Hook（含一墓碑）及一个临时 Worktree 绑定。
- 仅在合成夹具中预留四类序列高水位，模拟历史已删除 ID；迁移后新 Task/Note/History/Hook 的 ID 全部越过旧高水位。
- 保持 WAL 连接，通过旧 CLI 提交最后一条 Note。Backup API 含该 Note；故意仅复制主文件的负例缺失该 Note，证明不能采用主文件复制。
- 七表所有旧字段/BLOB/JSON/序列相等，新六表为空、project_id 为 NULL，完整性与外键通过；快照/复制阶段源主文件及 WAL 字节未变（不对易变 SHM 作字节不变承诺）。
- 新 CLI 读取 Task/Session/History/Import/context 和实时 Worktree；Checkpoint 后 Note 可见。墓碑重试仍 deleted，内容冲突仍拒绝。
- 新库项目关联不改执行 Session/状态/Worktree/Checkpoint；旧 CAS 拒绝。新旧 CLI 互开不兼容 Schema 均拒绝，未隐式升级。
- 0/1/3/4、未知布局、已有目标/三类 sidecar 拒绝；复制到 History 后注入异常，七表写入全部回滚，源快照不变。
- 完整演练连续两次退出 0，沙箱均已删除；`-O`/`PYTHONOPTIMIZE` 在创建夹具前拒绝，防止关闭断言产生假通过。Python AST、文档链接与 diff 检查通过。默认 Python LSP 的 ty/ruff 不可用，未获得 LSP 诊断；需要时安装它们或更新 `pi-lsp.json` 的命令配置。首次夹具调试修正了“open 不能直接 completed”以及 project create 的 `--name` 参数；未改变产品行为。

证据：`.local/task34-schema2-rehearsal-result.json`；旧构建日志 `.local/task34-schema2-build.log`。SQLite Python runtime 为 `3.50.4`。

| 二进制 | SHA-256 |
| --- | --- |
| 旧 Schema 2 taskctl | `3636625c2fba2e354574591befc6111a20b6e215c90f40631b302962916302b7` |
| 新 Schema 4 taskctl | `abc33f78eb77ab9df60a90ee5773ea64e6452ffe47636132616b1c0e1e31b29c` |

### 初版 Rust 入口的 Windows 验证（路径审查前）

- 全 workspace **179 Rust / 37 Node PASS**，含真实 Pi（未跳过）、build、Clippy `-D warnings`、fmt；七个相关 Rust 文件 LSP 零诊断。Python AST/实际运行替代不可用的 ty/ruff 诊断。
- 应用测试覆盖严格布局、损坏 JSON/BLOB/Hook 指纹、当前 Session 已结束、序列缺失/重复/低于 MAX、私有父目录要求、非普通 sidecar、目标占用、完整已提交 WAL 及复制途中源提交。
- 测试私有回调在复制中和发布前注入失败；真正启动测试子进程并在这两个边界强杀，断言源不变/目标不出现。再次复制不采纳或清理旧私有暂存。回调/环境变量只存在于测试入口，没有生产 CLI 故障开关。
- 真实 CLI 测试覆盖强制路径/确认、发布后拒绝重跑覆盖和公开读取。跨版本 Python 沙箱改用真实 Rust 命令后仍通过 10 组检查，七表计数与历史夹具一致；第十组现为真实入口覆盖说明，中断断言移到 Rust，而非继续测试 Python 内核。
- 真实 taskd 进程使用独立 DB/runtime、严格授权、127.0.0.1 临时端口、`--no-open`，在迁移目标上启动、读取 context、正常停机并重启，Task/History 不变；直接给新 taskd Schema 2 则在监听/创建 runtime 前拒绝。未打开浏览器或变更网络策略。
- 证据 `.local/task34-schema2-implementation-20260908-171639/`，含构建/测试日志和 `cli-rehearsal-final.json`。新构建 SHA 以该目录最后核验的 `verification.json` 为准，不能沿用上表原型二进制 SHA。

### 路径审查及修复后的 Windows 验证

- 审查发现 R1：`new.db.` / 尾随空格的目标返回成功且复制计数为 1，但用原参数或返回路径重开均读到另一空库；字面量发布文件仍包含 1 条 Task。R2：规范源旁存在 `-shm` 目录时，普通拼写拒绝，尾点/空格别名却绕过 sidecar 类型检查。均仅用合成数据复现，旧候选未冻结发布；证据 `.local/task34-migration-review-20260908-180200/`。
- 修复限于迁移路径验证与规范源 sidecar 检查。先确认新增路径单测在原实现失败，再修复为通过；CLI 负例确认拒绝后没有目标/私有目录/字面量别名残留，源字节及占用 sidecar 保持不变。
- 直接取得尾点/空格别名的原生规范身份，单独测试三类 sidecar 的规范名称检查，使此回归不被新的词法拒绝提前遮蔽；未模拟 8.3 环境或修改文件系统策略。正例覆盖中文/内部空格、普通/扩展路径，并用原参数和返回路径重开同一非空库。
- 完整 Windows workspace **184 Rust / 37 Node PASS**（含真实 Pi，未跳过），build、Clippy `-D warnings` 通过；本轮修改的三个 Rust 文件 LSP 零诊断。跨版本真实 CLI 演练仍为 10 组通过；全量 Rust 包含迁移后真实 taskd 启动/重启及强杀边界测试。未重跑真实 Chrome，也未执行 Linux 修复或迁移验证。
- 本轮证据 `.local/task34-migration-path-fix-20260908-184021/`；候选源码/构建身份以该目录最终 `verification.json` 与 `final-files.json` 为准，不沿用审查前的二进制 SHA。固定本地候选不等于提交、远程 CI、正式切换或业务验收。

**仍未验证/执行**：正式业务库及其权限、生产规模/内存上界、攻击性路径竞态、磁盘满与断电故障、已安装程序的成套切换、正式停写和恢复业务写入后的回滚。本轮隔离验证不等于正式环境切换获准。脚本始终不接受业务数据库路径，不可移除其合成边界用于正式库。

## 5. 推送后的 CI 与下一关

[CI run 34203459133](https://github.com/cecil-su/agent-steward/actions/runs/34203459133)，提交 `da8ae9b`：

- Formatting PASS；Ubuntu build/Clippy、172 Rust、Node 36 PASS/1 SKIP、真实浏览器两段 PASS。新增的是远程 Linux 自动化证据；此前本机 Linux/人工验收的用户 SKIPPED 记录不回写成 PASS。
- Windows build/Clippy PASS，但 `crates/server/tests/projects.rs` 两项 HTTP checkout 测试在第 283/363 行预期 200、实际 400（`select an advertised checkout`）。日志路径出现 `RUNNER~1`，短路径与 Git 公布路径的拼写差异是待复现假设，不能据此直接放宽预 IO 白名单。
- 总 CI **FAILURE**；Windows 后续 Node/launcher 未执行。没有禁用测试、修改 CI 来规避问题或自动重跑推送。

### 2026-09-08 本地修复与复验

- 本机临时目录没有现成 8.3 短名，未修改文件系统策略。仅对子测试进程设置大小写别名 TMP/TEMP，确认它与原目录是同一对象；未修改的测试复现相同两项 400 失败（3 PASS/2 FAIL）。这是同类路径拼写问题的复现，不是对 `RUNNER~1` 环境的原样复现。
- 修复仅在 `crates/server/tests/projects.rs`：从 fixture 仓库读取 `git worktree list --porcelain -z`，按已知、自有 fixture 的规范路径选定唯一 main/linked checkout，再向 HTTP 传入 Git 公布的拼写，不隐式选择首项。测试内的规范化不能移到生产请求路径上。
- 保留所有原负例，并补充 Windows 同对象未公布大小写别名仍拒绝、Project History 不变断言。生产路径校验、HTTP 白名单和 CI 配置未改。
- 修复后普通 TEMP、大小写别名 TEMP 的五项 Project HTTP 测试均 PASS；两种环境各自的完整 Windows workspace 均 **167 Rust PASS**，Clippy `-D warnings`、fmt 通过，目标 Rust 文件 LSP 零诊断。证据 `.local/task34-checkout-ci-20260908-164504/`。
- 尚未提交或再次推送；上述远程 `da8ae9b` CI 仍为 FAILURE，不能以本机通过替代新提交的远程复验。没有重跑 Node/Chrome：本轮仅修改 Rust 测试及本文，产品代码未变。

### 后续决定及当前下一关

测试修复已单独提交推送 `0d34375`，[CI run 34207573871](https://github.com/cecil-su/agent-steward/actions/runs/34207573871) 的 Windows 作业成功（167 Rust、Node36 PASS/1 SKIP、launcher/update safety）；Ubuntu Rust/Node成功，但浏览器点击“清除观测记录”超时，总体仍 FAILURE。用户明确暂缓 Ubuntu，保留测试/CI，不将该失败记 PASS，也不再以它阻塞本轮 Windows 工作。

迁移入口及 Windows 路径修复已提交 `0e32e04`；CI 的六处 Clippy 字节数组诊断经 `e4c5436` 修正，[CI 34222200849](https://github.com/cecil-su/agent-steward/actions/runs/34222200849) 整体成功，Windows 185 Rust、Node36 PASS/1 SKIP及启动器测试通过。本次 Ubuntu 作业也成功，不代表专项修复历史浏览器不稳定性或完成已跳过的人工验收。

只读盘点发现旧安装/打包链仍标注 Schema 2，因此现在补齐 Schema 4 安装兼容并做隔离验证：停止前只读预检、拒绝普通跨 Schema 切换、不自动回退不兼容旧程序、正确定位独立安装目录。用户确认其它 Pi 仅保留空闲会话，不要求关闭它们；最终快照前仍需明确维持停写，包括暂停当前会话的 Steward/Hook 写入。最终备份、正式库操作、安装/切换/发布及 Task 关闭仍需对应授权。

安装兼容当前工作区的 Windows 隔离验证：**188 Rust / 37 Node PASS**（含真实 Pi、无跳过），build/本地 Clippy/fmt 通过，五个相关 Rust 文件 LSP 零诊断，跨版本迁移演练 10 组通过。PowerShell 7.4.7 与 Windows PowerShell 5.1（仅测试子进程使用该宿主默认模块路径）均通过真实三件套打包/解包、taskd 启动/重启、错误 Schema 不停止现有服务、跨 Schema/不兼容回退拒绝、自定义目录与中文 JSON 路径测试；本地构建编排测试使用假 Cargo 输出。没有重跑可选的双次 release 重编译脚本或真实 Chrome，也没有修改全机模块路径/执行策略。首次完整 Rust 运行中断的日志不计通过，重新完整运行后取结果。证据 `.local/task34-schema4-launcher-20260908-213539/`，以最终 `verification.json` 的源码/二进制身份为准；这些安装兼容变更尚未提交，不能沿用 `e4c5436` 的 CI 结果。
