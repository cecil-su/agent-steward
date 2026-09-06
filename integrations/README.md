# Codex 与 pi 原生适配

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

## 时间、重试与验证边界

原生通知没有统一稳定投递 ID。两个适配器都为每次通知创建 `observation-UUID`；`occurredAt` 表示适配器观察时间，`receivedAt` 表示服务入库时间，不是宿主原始事件时间。内部 busy 重试复用同一 ID 和时间；宿主独立重复投递会生成另一条观察。不要用事件数量统计精确消息数，也不要重复配置适配器。需要端到端幂等时，由具有稳定事件 ID/时间的宿主使用通用协议。

只有白名单元数据入库；pi 在启动子进程前丢弃正文。Codex stdin 上限 256 KiB，超限报错并不采集，含较大工具结果的通知也可能因此丢弃。没有磁盘重试队列；每 Session 上限仍为 10,000 条。

```bash
cargo build --workspace --locked
node --test integrations/tests/native.test.mjs
```

测试使用真实 taskctl/task-hook 和临时 SQLite，验证负载映射、会话隔离、Task version 不变、正文未入库、pi 子进程失败回退、Codex 参数转义。pi 通过模拟扩展事件分发调用真实接收二进制，不是模型端到端测试。本机 Codex CLI 为 0.153.4；pi 尚未全局安装。真实客户端信任配置与模型运行应在实际使用会话中完成验收。
