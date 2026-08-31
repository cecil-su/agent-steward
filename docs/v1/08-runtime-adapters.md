# Herdr 与原生 Subagent Runtime

## 1. 目标

核心同时支持：

- Herdr 管理的独立终端 Agent；
- Pi、Claude、Codex 等宿主的原生 subagent；
- 人工打开并 attach 的外部 Session。

核心不依赖 pane、Tab 或某个宿主的私有 Session 格式。

## 2. Runtime Adapter 接口

```typescript
interface AgentRuntime {
  describeCapabilities(): RuntimeCapabilities;
  spawn(request: SpawnRequest): Promise<RuntimeHandle>;
  resume(request: ResumeRequest): Promise<RuntimeHandle>;
  sendPrompt(handle: RuntimeHandle, prompt: PromptArtifact): Promise<void>;
  getStatus(handle: RuntimeHandle): Promise<RuntimeStatus>;
  close(handle: RuntimeHandle): Promise<void>;
}
```

能力字段候选：

```text
canSpawn
canResume
canSendPrompt
canObserveStatus
canObserveSessionId
canSetCwd
canInjectCapability
canClose
canLayout
canNotify
```

## 3. Herdr Adapter

负责：

- 创建/恢复独立 Tab 根 pane；
- 设置 cwd 和最小环境；
- 观察 Session、pane、Tab 和工作/空闲状态；
- 布局和关闭；
- 将 Runtime Handle 与 Invocation 绑定。

Task Owner 先创建 Assignment/Grant，再调用 Herdr；不能先启动一个无主 Agent 后补登记。

## 4. Native Subagent Adapter

纯 CLI 无法通用控制所有宿主内建 subagent，需要 Host Plugin：

```text
Pi/Claude/Codex Host
  └── Steward Host Plugin
       └── taskd Runtime API
```

Host Plugin 负责：

- 将宿主 Session ID 映射到 Steward Session；
- 创建原生 subagent；
- 传递结构化 brief 和受限 MCP connection；
- 捕获完成/阻塞信号；
- 上报 capability。

V1 需要选定至少一个具体 Native Host 作为首个实现，并通过相同 Runtime conformance tests。

## 5. Manual Adapter

用户或 Agent 可在外部窗口执行 attach：

```bash
stewardctl session attach --grant <one-time-code>
```

一次性 code 只能接受既有 grant，不能指定新角色。

## 6. 状态对齐

Runtime 状态：

```text
starting / working / idle / closed / unknown
```

Assignment 状态：

```text
created / active / completed / blocked / cancelled / needs_reconciliation
```

两者不能互相替代：

- pane closed 不等于 completed；
- completed event 不等于业务验收；
- Runtime unknown 不自动释放 owner/writer lease；
- taskd 通过事件、artifact、Git 和 lease 进行 reconciliation。

## 7. Prompt 与报告

- Prompt 由模板、Task facts 和 Assignment scope 渲染为 PromptInstance。
- capability 不进入 Prompt 正文。
- 报告路径/artifact 必须由 Assignment 预先分配。
- 完成调用要求 idempotency key，并将 report hash 与 Assignment 绑定。

## 8. 适配器测试

每个 Runtime Adapter 必须通过：

- capability discovery；
- spawn/attach；
- cwd/scope；
- Session/Invocation 绑定；
- completion/blocked；
- crash/close；
- resume 或明确返回 unsupported；
- duplicate event 幂等；
- stale grant 拒绝。
