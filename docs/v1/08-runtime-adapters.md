# Herdr 与原生 Subagent Runtime

> 范围：本文属于可选阶段 D 的 Runtime 控制。阶段 C 只接入现有 AI 会话读写任务，不需要本 Adapter。只有手工启动/恢复成本被验证后才选择一个宿主；多 Runtime 和宿主上下文管理都不阻塞 V1 主线。

## 1. 目标

后续可评估的接入形态：

- Herdr 管理的独立终端 Agent；
- Pi、Claude、Codex 等宿主的原生 subagent；
- 人工打开并 attach 的外部 Session。

核心不依赖 pane、Tab 或某个宿主的私有 Session 格式。

## 2. Runtime Adapter 接口

以下是阶段 D 的候选接口，不是已有可执行 API。D-005 选择具体宿主后，按第 21 篇的能力/契约验证/当前环境/真实运行维度分别验收；阶段 C 读取已有任务不依赖本接口。


```typescript
interface RuntimeOperationContext {
  operationId: string;
  idempotencyKey: string;
  requestHash: string;
  orderingScope: string;
  effectSequence: number;
  attempt: number;
}

interface AgentRuntime {
  describeCapabilities(): RuntimeCapabilities;
  spawn(operation: RuntimeOperationContext, request: SpawnRequest): Promise<RuntimeHandle>;
  resume(operation: RuntimeOperationContext, request: ResumeRequest): Promise<RuntimeHandle>;
  sendPrompt(
    operation: RuntimeOperationContext,
    handle: RuntimeHandle,
    prompt: PromptArtifact,
  ): Promise<RuntimeEffectResult>;
  transitionContext(
    operation: RuntimeOperationContext,
    handle: RuntimeHandle,
    request: ContextTransitionRequest,
  ): Promise<ContextTransitionResult>;
  discoverHistory?(
    handle: RuntimeHandle,
    request: RuntimeHistoryDiscoveryRequest,
  ): Promise<RuntimeHistoryPage>;
  readRuntimeHistoryItem?(
    handle: RuntimeHandle,
    request: RuntimeHistoryReadRequest,
  ): Promise<RuntimeHistoryItem>;
  getStatus(handle: RuntimeHandle): Promise<RuntimeStatus>;
  close(operation: RuntimeOperationContext, handle: RuntimeHandle): Promise<RuntimeEffectResult>;
  reconcileOperation(
    operation: RuntimeOperationContext,
    knownHandle?: RuntimeHandle,
  ): Promise<RuntimeOperationReconciliation>;
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
canObserveContextWindow
canStartFreshContext
canCompactWithSummary
canResumeOpaqueContext
canDiscoverHistory
canReadRuntimeHistory
canReconcileOperation
supportsProviderIdempotency
```

上下文转换能力不能假设所有宿主一致。Adapter 必须明确声明支持 `fresh_window`、`summary_compaction`、`opaque_compaction`、宿主原生策略中的哪些模式；核心不能把任一 Codex、Claude、Pi 或 Herdr 的压缩实现写死为通用语义。

`transitionContext` 是 Runtime context-transition API；不支持请求模式时返回结构化 `RUNTIME_CAPABILITY_UNAVAILABLE`。`discoverHistory` 和 `readRuntimeHistoryItem` 只用于发现或导入宿主历史，Adapter 不成为 Steward 长期 History 的权威。

#### Runtime 外部副作用协议

1. taskd 先持久化 RuntimeOperation、规范化 canonical request、requestHash、idempotency key 和 `prepared` DomainEvent，再调用 Adapter。
2. Adapter 必须把稳定 operationId 传给支持幂等键/tag/metadata 的宿主；相同 operationId + requestHash 只能代表同一次副作用，不同 requestHash 返回冲突。
3. 调用前把 operation 标记 dispatching；成功后在 SQLite transaction 中保存 externalOperationId、RuntimeHandle/result 和 succeeded event。超时或响应丢失不能直接证明失败。
4. 重启或结果未知时调用 reconcileOperation。Adapter 优先按 provider operation ID/tag 查询；也可发现带 operationId 的既有 Session、Invocation 或已接收 Prompt。
5. 只有宿主明确证明“未执行”时才能用同一 operationId 重试。证明“已执行”则补记成功；无法判断则进入 needs_reconciliation，禁止重新 spawn、resume 或 sendPrompt。同一 orderingScope 的后续冲突操作也必须被 taskd 阻止，不能用新 idempotency key 越过未决 operation。

Runtime conformance tests 必须覆盖宿主成功后 taskd 崩溃、响应丢失、重复请求、同 operationId 不同 requestHash，以及无法 reconcile 的降级路径。声称 canReconcileOperation 的 Adapter 必须通过这些测试；不支持 provider idempotency 的 Adapter 不得把未知结果伪装成可安全重试。

taskd 另外提供与 Runtime Adapter 分离的应用接口：

```typescript
interface HistoryQueryService {
  listWindows(query: HistoryWindowQuery): Promise<HistoryWindowPage>;
  listItems(query: HistoryItemQuery): Promise<HistoryItemPage>;
  readItem(query: HistoryReadQuery): Promise<HistoryItem>;
  search(query: HistorySearchQuery): Promise<HistorySearchPage>;
}

interface WorkingNoteService {
  list(query: WorkingNoteQuery): Promise<WorkingNotePage>;
  read(query: WorkingNoteReadQuery): Promise<WorkingNote>;
  create(command: WorkingNoteCreateCommand): Promise<WorkingNote>;
  append(command: WorkingNoteAppendCommand): Promise<WorkingNote>;
  write(command: WorkingNoteWriteCommand): Promise<WorkingNote>;
}
```

HistoryQueryService 只查询 taskd 已纳管的本地 History；WorkingNoteService 通过 Command/Query、授权、幂等和版本检查读写本地权威存储。

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

若选择 Native Host 路线，先实现一个具体宿主并通过 Runtime conformance tests；不要求与 Herdr 同时交付。

## 5. Manual Adapter

用户可在外部窗口执行 attach，可信 Host 也可为 Agent 建立受保护输入通道：

```bash
stewardctl session attach
```

命令不得接受包含 secret 的 `--grant <value>`，也不得从 Prompt 或可继承环境变量读取。交互模式从关闭回显的终端 stdin 读取；自动模式只接受受保护本地管道或可信宿主传入的继承句柄。一次性 capability 只能接受既有 grant，不能指定新角色，并绑定 audience、目标 Session/Assignment、TTL 和单次消费 nonce。

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

## 7. 按所选 Runtime 需要启用上下文窗口与工作记忆

- 一个 Session 可以包含多个 ContextWindow；窗口切换不创建新的 Task，也不改变 Assignment 结论。
- 手动 reset、自动 token-budget 切换、模型变化和 resume 统一记录持久化 transition operation/event；Adapter 内部实现可以不同。
- 如果 Runtime 支持切换前通知，taskd 可以请求 WorkingNote/ContextCheckpoint；即使 Agent 未及时写入，taskd 仍记录当前 Task 版本、owner epoch、Artifact 和 Git 引用，并标记 checkpoint 是否完整。
- 新窗口必须重新读取当前 Task、Assignment、grant、owner epoch 和已确认业务事实，生成新的 ContextBrief；不得盲目恢复旧窗口的完整 Prompt 或过期权限。
- History 只读接口按 Session/Assignment scope 授权，并限制搜索范围、返回大小和单次读取量。
- WorkingNote 使用由 taskd 分配的逻辑名称和存储对象，不接受任意文件路径；create 必须带 Assignment、logicalName 和幂等键，并保证同一 Assignment 内 active logicalName 唯一；append/write 必须带幂等键、Note expectedVersion 和 current content Link expectedVersion，正文通过 Artifact publish-before-reference 协议发布。
- History 和 WorkingNote 只能提供执行证据与工作记忆，不能直接写 Task、ReviewDecision、Requirement、Decision、Preference 或 live Prompt。
- principal、session、assignment 和 runtime identity 由可信 connection 注入，不能由普通 history/notes tool arguments 覆盖。

## 8. Prompt 与报告

- Prompt 由模板、Task facts 和 Assignment scope 渲染为 PromptInstance。
- PromptInstance 绑定 ContextWindow 和 ContextBrief 版本。
- capability 不进入 Prompt 正文。
- 报告路径/artifact 必须由 Assignment 预先分配。
- 完成调用要求 idempotency key，并将 report hash 与 Assignment 绑定。

## 9. 适配器测试

每个 Runtime Adapter 必须通过：

- capability discovery；
- spawn/attach；
- cwd/scope；
- Session/Invocation 绑定；
- completion/blocked；
- crash/close；
- resume 或明确返回 unsupported；
- 支持的 context transition mode 或明确返回 unsupported；
- 手动与自动窗口切换产生一致的 lifecycle event；
- model/config/skill/environment 变化后重新生成 ContextBrief；
- resume 不重复注入 initial context，也不恢复过期 grant/owner epoch；
- History/WorkingNote scope、幂等、版本冲突和输出边界；
- duplicate event 幂等；
- stale grant 拒绝。

## 10. 恢复与重试的实施前检查

D-022 尚未关闭：普通业务请求命中 Receipt 时应使用持久目标重新鉴权，不能重新解析 active Run 后误操作替代执行。第 05 篇的 active Run 选择仅能描述首次调用，最终请求标识和重试匹配顺序必须在实现控制命令前冻结。

需要区分同一未知操作的重放、已失败操作后的显式新尝试和更换 Runtime/目标的独立执行。未知结果先 reconcile；不能以新 idempotency key 绕过同一 orderingScope 的未决操作。已终结执行不因重启扫描恢复为 active。只有未来确有多次真实调用的追溯需求，才评估如何细化既有 RuntimeOperation/AgentRun，不直接增加一套 Run/Attempt 模型。

该原则参考 [Xuanwu 的恢复合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0069-restart-recovery-invariants.md)，不复制其调度、自动重试预算或通知框架。验收必须包括执行成功后回执丢失、重复重启核对、替代 Run 出现后旧请求重试，以及无法证明是否执行的停止自动动作路径。
