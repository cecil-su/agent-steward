# Codex 与 pi 原生适配

#34 当前开发 Schema 4 只用于隔离新库；未获迁移/部署授权前，不按下列部署示例覆盖正式 CLI/Hook。新增上下文采集器仅为仓库内实验模块，见下文。

先运行 `cargo build --workspace --locked`。适配器需要显式绑定，不读取 transcript，不保存消息正文，不改变 Task 执行状态。

## 绑定

从客户端取得实际会话 ID，再使用最新 Task version：

```bash
taskctl --database /absolute/steward.db --json task show TASK-1
taskctl --database /absolute/steward.db --json session bind local-a \
  --source codex --external-session ACTUAL-CLIENT-SESSION-ID --if-version VERSION
```

pi 使用 `--source pi`；每个客户端需要独立本地 Session。外部 ID 不是会话文件路径。切换到其他外部会话不会自动改绑，适配器会跳过不匹配的会话。

## Codex

依据 [Codex 官方 Hooks](https://learn.chatgpt.com/docs/hooks)，生成 hooks.json 配置片段：

```bash
python3 integrations/codex/configure.py \
  --binary /absolute/agent-steward/target/debug/task-hook \
  --database /absolute/steward.db --session local-a \
  --external-session ACTUAL-CLIENT-SESSION-ID > /tmp/steward-hooks.json
```

把生成条目合并到项目 `.codex/hooks.json`，保留已有 Hook，不重复安装。生成器只输出 JSON，不修改客户端配置或信任。项目需要受信任；在 Codex `/hooks` 中审阅并信任新增/变更的 Hook，然后继续绑定的会话。Hooks 功能须启用。生成器采用 POSIX shell 参数转义，面向 macOS/Linux。

| Codex 事件 | Steward 观测 |
| --- | --- |
| SessionStart startup | started |
| SessionStart 其他来源 | resumed |
| UserPromptSubmit | user_message |
| PreToolUse / PostToolUse | tool_call / tool_result |
| Stop / Interrupt | idle |
| SessionEnd | closed |

不订阅权限决策或子 Agent 专用事件。Stop 不代表 Task 完成；SessionEnd 不关闭本地执行 Session。工具覆盖范围取决于客户端官方 Hooks 支持。成功或跳过事件仅输出 `{}`；失败退出 1，stderr 仅输出固定类别错误，不返回阻断/继续指令，不使用退出码 2。每个 Hook 设置 3 秒上限。

## pi

入口 `pi/steward.ts` 与同目录 `steward.mjs` 必须一起保留。接口依据上游 `@earendil-works/pi-coding-agent` 0.85.1 的 [扩展文档](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md)。可先用 `pi -e /absolute/agent-steward/integrations/pi/steward.ts` 加载扩展，在客户端运行 `/steward-session` 显示实际 ID，完成显式绑定后设置下面环境变量并恢复该会话。该命令只读取会话 ID，不读取会话文件。

```bash
STEWARD_HOOK_BIN=/absolute/agent-steward/target/debug/task-hook \
STEWARD_DATABASE=/absolute/steward.db \
STEWARD_SESSION=local-pi \
STEWARD_EXTERNAL_SESSION=ACTUAL-PI-SESSION-ID \
pi -e /absolute/agent-steward/integrations/pi/steward.ts
```

使用 pi 的恢复选项继续已绑定的会话。长期使用可按上游扩展配置添加入口路径，遵守 pi 自身的信任流程。环境变量在加载时读取，修改后重新加载。未配置完整时提示一次并不采集。

| pi 事件 | Steward 观测 |
| --- | --- |
| session_start startup/new/fork | started |
| session_start resume/reload | resumed |
| message_end user/assistant | user_message / assistant_message |
| tool_execution_start | tool_call |
| tool_execution_end | tool_result，isError 时为 error |
| agent_settled | idle |
| session_shutdown | closed |

使用 agent_settled 避免把自动重试、压缩或后续消息之前的 agent_end 当作空闲。session_shutdown 的 reload/new/resume/fork 也只是旧扩展生命周期结束。处理器返回 undefined，不替换消息或工具结果。子进程使用参数数组，不使用 shell；单次等待最多 2 秒，失败显示一次固定提示，手工 CLI 仍可使用。

## #34 Pi 上下文证据采集（实验模块，不自动加载）

`pi/context-evidence.mjs` 导出 `createPiEvidenceCollector`，没有默认扩展入口，不注册工具或命令，不修改已有 steward.ts/steward.mjs。依据本机 Pi 0.85.1 随包 README 的 Context Files、docs/extensions.md、docs/sdk.md、docs/session-format.md、docs/environment-variables.md 及 examples/extensions/prompt-customizer.ts、examples/sdk/07-context-files.ts 核对接口。

- `beginSession(ctx)` 在当前实例启动/重载后建立新随机实例身份；shutdown 调 `invalidate()`。Session/CWD 改变、未初始化或已失效时拒绝采集。调用方不得继续使用旧 ctx；不读取 Session 文件或聊天记录。
- `collectBase(ctx)` 使用**命令上下文**的 `getSystemPromptOptions()`；`collectBeforeAgentStart(event,ctx)` 使用事件的 systemPromptOptions/systemPrompt。缺失接口/字段不回退到猜测文件列表。
- 保留实际 contextFiles 顺序，只输出报告路径、加载文本 UTF-8 hash/字节数。支持虚拟规则路径，不做磁盘发现。custom/append/chained prompt、skills/工具提示元数据仅保留 hash，不返回正文，不读取鉴权、环境变量全集或 settings/auth 文件。
- 使用 `getActiveTools()`/`getAllTools()` 读取当前选择和配置目录元数据；不调用工具、不改变 active 集合。描述/schema/sourceInfo 的摘要叫 metadataSha256，**不是实现版本或运行配置证明**；implementationVersion=null、configurationVerified=false。选择集合与提示输入不一致即拒绝。
- 双次同步快照不一致、时钟回退/采集超时、重复/未知工具、超限或异常均返回固定错误，不带原始异常/正文。64 个规则、256 个工具；规则256 KiB/项及4 MiB合计；JSON元数据128 KiB/项及工具合计2 MiB；输出最多64000 UTF-8 bytes，不静默省略。
- 观察生命周期最多60秒，fingerprint绑定版本/实例/Session/CWD/采集阶段/加载内容及工具元数据；返回 expiresAtMs 不构成复用许可。hostClaimVerified/reuseAllowed恒false，无磁盘缓存、数据库写入、消息注入、日志、子进程或网络操作。

**权威边界：** Pi 默认每目录优先 AGENTS.override.md，否则 AGENTS.md，再否则 CLAUDE.md；global/祖先/CWD 规则按加载顺序合并，`--no-context-files` 可以禁用。SDK还可替换/提供虚拟contextFiles。因此 Rust 的宿主无关文件导航不等于 Pi 已加载清单。`getSystemPromptOptions()` 只表示基础输入，before_agent_start 只表示当前处理器阶段；后续处理器、context、before_provider_request 仍可改写，不能称为最终发给模型的规则全集。

**不直接转换为 Rust HostEvidence。** 加载文本是 Pi 解码后的字符串，不是本机原始字节/对象身份；Pi Session ID 也不是 Steward Session ID。后续须显式绑定当前请求与本机身份，核对加载内容和规则覆盖，不能填造 objectSha256、把元数据摘要充当工具配置摘要，或用 Pi 版本冒充每个工具版本。当前 native identity/config/final payload/rule scope/dependency blocker 均保留。

验证：

```bash
# 无运行时依赖的单元测试；真实 Pi 子测试需显式提供二进制路径。
node --test integrations/tests/pi-context.test.mjs
# 本机执行过的隔离验证（不安装、不调用模型）：
STEWARD_TEST_PI_BIN=/absolute/pi.exe node --test integrations/tests/pi-context.test.mjs
```

`tests/pi-context-probe.ts` 仅供测试显式 -e 加载，不复制到扩展发现目录。测试创建临时 agentDir/HOME/CWD、仅继承OS进程必要环境，禁用网络启动、持久会话、项目资源、其它扩展/skills/prompts/themes；命令处理器直接采集，不发送模型 prompt。Pi 0.85.1 真进程验证了 override 优先级、AGENTS 优先于 CLAUDE、禁用 contextFiles、active read 选择和未持久化会话；其它事件生命周期用模拟 API 测试。没有验证真实会话的完整扩展链、最终 provider payload、工具内部配置或业务执行权限。

## 时间、重试与验证边界

原生通知没有统一稳定投递 ID。两个适配器都为每次通知创建 `observation-UUID`；`occurredAt` 表示适配器观察时间，`receivedAt` 表示服务入库时间，不是宿主原始事件时间。内部 busy 重试复用同一 ID 和时间；宿主独立重复投递会生成另一条观察。不要用事件数量统计精确消息数，也不要重复配置适配器。需要端到端幂等时，由具有稳定事件 ID/时间的宿主使用通用协议。

只有白名单元数据入库；pi 在启动子进程前丢弃正文。Codex stdin 上限 256 KiB，超限报错并不采集，含较大工具结果的通知也可能因此丢弃。没有磁盘重试队列；每 Session 上限仍为 10,000 条。

```bash
cargo build --workspace --locked
node --test integrations/tests/native.test.mjs
```

原生测试读取 `CARGO_TARGET_DIR`（未设置时仍为 target）下的 debug 构建，允许使用 #34 隔离构建目录，不调用全局 taskctl。CI及Windows发布入口还包括 pi-context.test.mjs；真实Pi子测试仅在显式提供二进制时执行，不因单元测试通过宣称真实会话验收。

测试使用真实 taskctl/task-hook 和临时 SQLite，验证负载映射、会话隔离、Task version 不变、正文未入库、pi 子进程失败回退、Codex 参数转义。pi 通过模拟扩展事件分发调用真实接收二进制，不是模型端到端测试。本机 Codex CLI 为 0.153.4；此处原生 Hook 未在本批安装到真实会话。真实客户端信任配置与模型运行应在实际使用会话中完成验收。
