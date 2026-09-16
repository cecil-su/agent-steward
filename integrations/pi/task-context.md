# Pi / Herdr 任务上下文候选入口

## 接入位置与范围

`task-context.ts` 是显式加载的 Pi 扩展入口，`task-context.mjs` 实现 `session_start` 会话边界、`input` 预检门控及展开后的 `context` 注入。每次输入先调用真实 CLI `task context --require-read-only`，完整保留 JSON envelope、任务需求、sessionRules 正文/ID/revision/来源和 warnings；不改写输入文本或图片。Pi 完成 `/skill:*` 与 prompt template 展开后，在模型调用前附加瞬时上下文消息，不改变模板参数语义。

合同边界为数据库 **Schema8**、CLI/HTTP **envelope3**、UI合同5；三个版本号不可混用。本入口读取任务、项目及规则的已登记资料，不是已删除的 `project context` 源码读取接口，不探测 Git、不读取登记的源码目录。业务状态为 backlog/todo/in_progress/in_review/blocked/done/cancelled，状态和 Session 关联不构成执行权限。

这是仓库内候选接入，不是全局部署。不修改运行中的 `C:/Users/shuxingxing/.pi/agent/extensions/agent-steward.ts`。另有独立的[管理候选](agent-steward.md)，只读入口不调用其写工具，也不接入自动 Session 发现。旧 subagent-handoff/Markdown 管理入口不使用。

Herdr 的 `agent start ... -- AGENT_ARG...` 将参数传给实际 Pi；手工终端直接运行相同 Pi 参数，二者使用同一个 input 门控和同一条 context 查询。候选不建立任务/会话注册表，不调用 claim、resume、bind、appendEntry 或任何 Steward mutation。读取上下文不构成领取、业务状态变更、部署或远程写授权。仓库 AGENTS 独立由 Pi 读取。

## 候选启动

仅在明确指定的、没有任务关联的新窗口/新进程加载；可执行文件须为可信的兼容版本，数据库必须已经存在。参数不回退 PATH 中的 taskctl 或默认库。

手工窗口（用自己的隔离数据库及返回的数字任务 ID 替换占位符）：

```text
pi --no-extensions -e ABSOLUTE_REPOSITORY/integrations/pi/task-context.ts --steward-context-cli ABSOLUTE_CANDIDATE/taskctl.exe --steward-context-database ABSOLUTE_SYNTHETIC_DB --steward-context-task TASK_ID
```

Herdr（已有空闲 shell pane，NAME/PANE 从实际环境取得）：

```text
herdr agent start NAME --kind pi --pane PANE -- --no-extensions -e ABSOLUTE_REPOSITORY/integrations/pi/task-context.ts --steward-context-cli ABSOLUTE_CANDIDATE/taskctl.exe --steward-context-database ABSOLUTE_SYNTHETIC_DB --steward-context-task TASK_ID
herdr agent prompt NAME "明确的任务请求"
```

`--no-extensions` 只关闭自动发现扩展，仍加载显式 `-e` 候选；这样不把候选新 schema 接到已安装管理扩展的默认旧库。保留 Pi 自身的项目信任和适用 AGENTS 行为，不自动授予信任；宿主提示信任时由操作者决定。不要将这些命令用于接管已有工作 pane。

本入口仅支持全新进程中的新会话。已有聊天、`/new`、`/resume`、`/fork`、`/reload` 不自动继承目标，启用参数但会话不匹配时拦截输入；需要针对另一个任务重新启动明确指定的新进程。没有指定任何候选参数时扩展不介入。部分参数、非正数字任务、非绝对路径、缺失数据库均拒绝，不创建空库。

## 读取与失败行为

`task context` 使用 SQLite READ_ONLY|NOFOLLOW 连接与 query_only，不经过建库、chmod、journal_mode=WAL 初始化路径。本地路径、普通文件、sidecar与打开前后文件身份检查沿用只读 preflight。缺失路径、空文件、非空 SQLite Schema0、无效/旧版数据库均失败，不创建父目录、主库或业务表；合法库仍读取已提交 WAL。`--require-read-only` 是宿主兼容性断言，旧 CLI 必须在参数解析阶段拒绝，不能回退无该参数的调用。

只读不等于零文件系统活动：SQLite 对合法 WAL 库可能创建/更新 `-shm`，需要相应目录权限；不使用 `immutable=1`、忽略 WAL 或复制运行中主库来规避。主库和 WAL 无业务写入，不设置文件权限，不绕过 OS 权限失败。空库/Schema0/无效库测试核对字节及目录项不变；合法 WAL 测试核对主库/WAL、data_version与业务历史不变。

每次 input 都重新查询，无长期缓存。只接受 schemaVersion:3、成功 envelope、匹配的数字任务、有效 Task version、七种有效业务状态、sessionRules.formatVersion:1、active 规则；规则内容当前支持 contentVersion:1，验证 ID/revision、scope/projectId、正文和历史来源。跨项目或候选/停用条目混入成功响应也拒绝。原始输入由 Pi 正常展开，完整上下文在 `context` 阶段独立交付。该消息不写入持久会话历史；每次模型调用只附带当前输入通过预检的快照。

CLI 调用不经过 shell；15秒超时，stdout 最多8 MiB、严格 UTF-8/JSON。超限明确失败，不截断必读内容。查询前和 IO 后复查捕获的 Session ID/cwd；切换过程中不会把旧请求结果交给新会话。

读取/验证失败通过 `input: {action:"handled"}` 阻止该输入进入模型，输出 `STEWARD_CONTEXT_UNAVAILABLE`，不注入空规则或继续旧缓存；通知异常不改变阻断结果。注入阶段再次检查身份；缺快照或 Session/cwd 不匹配时立即触发当前 Agent abort signal，不等待自身循环 idle，也不注入旧内容。不能只在 before_agent_start 抛异常：Pi 会记录扩展异常后继续执行。真正空规则则成功注入 `rules:[]`，与失败不同。Pi print模式的正常退出码不代表任务处理成功，自动化须同时处理此错误诊断；Herdr idle/done 也不代表任务完成。

上下文可含用户偏好及任务资料，会进入被选模型；瞬时规则消息不由本入口写入 Pi 持久历史，但模型响应、其他扩展或 provider 日志可能保留派生内容。本入口不额外落盘“已阅读”状态或扫描全部聊天。操作者负责模型目的地和资料敏感性；测试使用 `--no-session` 与隔离本地合成 provider。

## 验证

```text
STEWARD_TEST_BIND=172.19.10.185 CARGO_TARGET_DIR=ABSOLUTE_RULES_TARGET STEWARD_REQUIRE_REAL_PI=1 STEWARD_TEST_PI_BIN=ABSOLUTE_PI_0.85.1 fnm exec --using=24.11.1 node --test integrations/tests/task-context.test.mjs
```

- Rust CLI创建合成任务A反馈、global/project/candidate/disabled规则；真实 Pi CLI加载候选扩展，新建独立 Session，将请求交给实际 Agent循环。
- 本地临时端口的合成 OpenAI兼容provider接收最终模型请求并返回固定文本；断言最终请求完整匹配CLI envelope，不调用外部模型、不启用写工具。
- 多个真实新进程/不同Pi Session ID验证：任务B获得初版规则；A反馈修正规则后，新B会话获得新revision/完整来源；跨项目和无项目隔离；停用后排除；真正空规则成功。
- 技能及模板保持启用，以相同 `/skill:reviewprobe alpha`、`/reviewtemplate alpha "two words"` 对比无门控基线：展开正文与参数逐字相同，启用门控时另有完整规则快照；普通输入也不改写。
- 不存在任务/数据库、旧 Schema6/7 库、当前不支持的内容格式2、注入前身份变化均不发出模型请求，错误不伪装成空集合；缺库时技能与模板请求也阻断。Task/Project/Session/History不变，旧库字节不变。
- 门控单测覆盖 Session/cwd 更换、IO 途中切换、恢复会话拒绝；独立 CLI 读取回归覆盖空文件、两类非空 Schema0、Schema6/7、无效与缺失文件、合法 Schema8 库及缺任务。
- 解码单测接受 envelope3 的全部七状态，拒绝 envelope2、未知 envelope 版本、旧状态及无效任务版本。真实 CLI 隔离测试验证每种业务状态都可只读取得资料，Task/History/Session 不变，不生成 worktreeStatus。
- Schema8/envelope3 的 Windows 本地隔离验证通过：6 项测试全部通过，包括现有 Pi 0.85.1 新进程与合成 provider 的最终请求检查；HOME/数据库隔离，未安装或修改全局扩展。没有指定 `STEWARD_TEST_PI_BIN` 时真实 Pi 项明确跳过，不能将跳过计作通过。真实 provider 与 local-preview 测试读取 `STEWARD_TEST_BIND`。本机验证必须显式设置 `172.19.10.185`；CI 未设置时保留 `127.0.0.1` 默认值。指定地址不可用时失败，不自动回退。

### CI 与 Windows 发布门禁

两条 workflow 均运行 `integrations/tests/*.test.mjs`，包含 task-context 和预览代理合同测试。CI Windows及Windows发布检查**必跑真实Pi**：`.github/scripts/install-test-pi.ps1` 在新的 runner 临时目录安装固定0.85.1 Windows x64发行包，校验 SHA256 `002fa95b90d521245b9985d8f168caebc237ad56e7e30b319807dee1b2e17e1c`及实际版本，显式设置二进制路径与 `STEWARD_REQUIRE_REAL_PI=1`；缺路径直接失败，不接受skip。安装不修改PATH或全局配置。

CI Linux矩阵只承诺无Pi单测，真实宿主项显式skip；不可计入真实宿主覆盖。本地未设置必跑标记与Pi路径时也只运行单测。真实宿主覆盖仅由上述必跑Windows检查及明确指定Pi的本地执行提供；未执行远程CI不等于发布检查已通过。

实际 Herdr 交互 pane 的人工启动仍未验收。真实 Pi 模型请求路径仅验证了隔离新进程和合成 provider，不代表正式工作流验收；候选加载之外的已安装全局入口没有自动升级。

## 全局接入的外部依赖

如果需要无需候选参数就支持已关联任务的自动注入，最小外部变更位置为上述安装版 `agent-steward.ts`：将 `activeTaskId`、实际CLI路径和明确数据库配置交给本模块的 `readTaskContext`，在可阻止输入的事件阶段处理失败，并完整交付context；保留已有关联选择与Task CAS语义，不引入自动claim或任务扫描规则归纳。

安装扩展的实际配置入口是 `AGENT_STEWARD_TASKCTL` 和 `AGENT_STEWARD_DATABASE`；未指定时分别回退到PATH中的taskctl和默认库。正式接入前必须显式配置与核验二者，不能只依赖 JSON envelope 版本3 判断规则能力。该变更必须与 Schema8 候选 CLI/数据库配置一起验证，不能仅替换全局扩展而仍连接正式旧库。需要对外部源码维护及部署另行明确授权；当前未修改安装文件、全局配置或正式CLI/taskd。
