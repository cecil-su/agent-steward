# 待决策事项

这些问题在实现对应阶段前必须关闭。未关闭不代表 V1 文档无效，但不得在代码中静默选择。决策按 Workspace/Repo → Task → AI → Knowledge 顺序关闭；Phase 1 Registry 不应被后续 Task、AI、Git 写入或 Optimizer 决策阻塞。

## D-001 技术栈

候选：

- TypeScript/Node：MCP 和现有生态集成快，但需要管理 Node 版本与单文件分发；
- Go：单二进制和服务部署友好，Runtime/MCP 生态需要评估；
- Rust：安全与单二进制强，开发和适配成本较高。

需要 capability spike，而不是只凭偏好决定。

## D-002 taskd 运行身份

- Standard：与用户同身份；
- Hardened：独立 OS 用户/服务、容器或沙箱。

需要明确 Windows、macOS、Linux 的最小可行方案。

## D-003 用户审批渠道

需要选择 Agent 无法伪造的渠道：

- 本地 Web UI；
- 系统托盘/桌面 UI；
- 独立可信终端；
- OS credential/生物识别。

普通 Agent 可执行的 CLI 命令不能单独作为 Hardened 批准。

## D-004 Git 权限默认值

需通过实际用户使用确定 commit、merge、push 的默认 deny/ask/allow 策略。无论默认值如何，force push、reset hard 和 destructive clean 首版建议 deny。

## D-005 Native Host 首选实现

Herdr 必须支持；Native Runtime 首个具体宿主需从 Pi、Claude Code、Codex 中选择。核心和 SDK 必须允许后续扩展。

## D-006 数据加密

- SQLite 是否使用 SQLCipher；
- Blob 加密格式；
- OS Keychain 与用户口令；
- 全文搜索/embedding 与加密之间的取舍；
- 密钥丢失策略。

已冻结的备份最低要求：可移植备份必须带由用户备份口令/恢复密钥或显式外部 key provider 包装的加密 key envelope；不得保存明文密钥，也不得把仅对原设备有效的 OS Keychain 引用宣称为可移植恢复材料。具体 KDF、轮换和丢失处置仍需在本决策中关闭。

## D-007 Session 全量采集方式

不同宿主的 Session 格式、增量读取、附件和 compaction 表达不一致，需要定义 importer contract 和 provenance schema。

## D-008 Optimizer 使用的模型

- 当前 Agent 模型；
- 本地模型；
- 用户指定云模型。

任何云模型读取 Session 都必须是单独、可见、可撤销的授权，不属于默认无遥测行为。

## D-009 数据保留与删除

长期数据保留策略仍需决定：

- 默认加密和存储上限；
- 删除后的审计最小事实；
- embedding/全文索引重建；
- 用户偏好在证据删除后的处理。

## D-010 产品和组件命名

暂定：

- 项目/仓库：`agent-steward`
- 服务：`taskd`
- CLI：`stewardctl`
- MCP：`steward-mcp`

公开前检查 GitHub、npm、PyPI、crates.io、Homebrew、Scoop 和可执行文件名冲突。

## D-011 开源与插件边界

需要确定：

- License；
- Core/Adapter/Policy Pack 的仓库结构；
- NRS 等私有规则不得进入通用 Core；
- 第三方 Adapter 的权限与签名机制；
- 数据 schema 和 plugin API 的兼容策略。

## D-012 Markdown 迁移

SQLite 成为权威前，需要一次性 importer、只读 shadow 对比和切换计划。切换后 Markdown 仅由 exporter 生成，不能长期双向维护。

## D-013 Task 最小模型与生命周期

实现任务内核前需要冻结：

- Ready 是否强制要求 owner、nextAction 和 acceptanceCriteria；
- Blocked 恢复到 Ready 还是此前状态；
- Done 重新打开的目标状态已冻结为 In Progress；旧 ReviewSubmission/Decision 仅保留历史，再次验收必须创建新的 reviewCycle。仍需决定 reopen 对完成率、周期时间等指标的统计处理；
- ReviewSubmission 的 evidence 容量/保留上限；Task/criteria/evidence 版本绑定和 accept 原子事务已冻结；
- archive/restore 的授权、允许状态和默认视图行为；
- 父子任务的完成约束和阻塞循环检测策略。

建议用 20–30 个真实任务样本做状态迁移演练后决定。

## D-014 TUI 与 GUI 交付策略

需在同一代码库内选择：

- 同时交付薄客户端；
- 先交付 TUI，再复用契约实现 GUI；
- 先交付 GUI，再补齐 TUI。

无论顺序如何，两端必须共享 Command/Query/Event contract，不允许共享数据库文件但各自实现业务规则。

## D-015 DomainEvent 与同步策略

已冻结的最小 envelope 与同步协议：

- eventId、全局唯一且单调递增的 streamPosition、aggregateType、aggregateId、aggregateVersion；streamPosition 与 aggregateVersion 分离并允许空洞；
- principalId、actorId、eventType、payload；
- correlationId、causationId、idempotencyKey、requestHash、createdAt；
- 必填正整数 schemaVersion，按 eventType 标识 payload schema。未知版本必须显式 upcast 或拒绝，不能猜读。
- Snapshot Query 必须在同一个 SQLite read transaction 返回 read model + eventWatermark；客户端只消费 `streamPosition > watermark` 的事件。
- cursor 早于 outbox earliestAvailablePosition 时返回 CURSOR_EXPIRED 并强制重取 snapshot，不能静默跳到仍保留的事件。
- ArtifactBlobPending 是 internal/audit-only 存储记录，没有业务 streamPosition，不进入普通客户端 Event Stream。

仍需决定：

- TaskEvent、FactRevisionEvent、MappingEvent、SnapshotEvent 和 DriftFindingEvent 的事件族与兼容规则；
- outbox 保留期限、压缩方式与 earliestAvailablePosition 的发布方式；
- 是否在首版提供实时推送，还是先用可靠轮询；
- 更正和删除在不可变时间线中的表达方式。

## D-016 AI 接入边界

进入 AI 阶段前需要冻结：

- 首个 Runtime 与最小 Adapter contract；
- AI 作为 Owner、Worker 或两者都支持；
- Assignment 与 Task/Subtask 的边界；
- AI 提交 Review 所需最小证据；
- 哪些操作只需 task policy，哪些需要安全 capability 和可信批准。

## D-017 AI Context Window 与持久工作记忆

进入首个 AI Runtime 垂直切片前需要冻结：

- 验证“一 ContextWindow 只服务一 Assignment”，以及 Session 切换 Assignment 时关闭旧窗口并创建新窗口的约束；
- 首个 Runtime 支持 `fresh_window`、`summary_compaction`、`opaque_compaction` 或宿主原生策略中的哪些模式；
- 手动 reset、自动 token-budget 切换、模型变化和 resume 是否使用同一 transition event；
- 窗口切换前最小 ContextCheckpoint 的字段，以及 checkpoint 不完整时的恢复行为；
- History 的采集范围、只读查询、搜索、截断、保留和删除语义；
- WorkingNote 的逻辑命名、scope、版本、幂等、覆盖和 supersede 规则；
- 新 ContextBrief 必须重新读取的 Task、Assignment、授权、业务事实和 Git 版本；
- model/config/skill/environment fingerprint 变化后的重建规则；
- History/notes 默认本地存储，接入远程后端时的显式授权和数据范围；
- History、WorkingNote、ContextCheckpoint 和正式 Task/Business Fact 之间的权威优先级；
- resume、重复 initial context、过期 owner epoch、越权 history 查询和 notes 冲突的验收测试。

核心原则是允许模型上下文随时重置，但 Task、授权和已确认业务事实必须保存在窗口之外；History 和 WorkingNote 只能帮助恢复执行，不能成为第二套状态权威。

## D-018 Workspace / Repository Identity

进入 Phase 1 前需要冻结：

- Workspace.rootPath 是 discovery scope、显示属性还是安全边界，以及 root 移动后的 reconcile；
- Repository identity 如何组合 canonical real path、Git common dir、remote identity 和无 remote Repo；
- Windows 大小写、junction/symlink、UNC，macOS case folding，以及跨平台备份恢复后的 path 规范化；
- nested Repo、submodule、bare Repo 和 linked Worktree 的发现/确认策略；
- moved、missing、identity_conflict 与重新发现的状态转换；
- dirtyState 的读取范围与性能上限；
- unlink/archive 只改变 registry 状态、绝不删除或修改用户目录的验收测试。

## 建议优先级

Phase 1 Registry 编码前优先关闭：

1. D-018 Workspace / Repository Identity；
2. D-014 首个 TUI/GUI onboarding 交付策略；
3. D-001 技术栈；
4. D-015 DomainEvent 与同步策略；
5. D-012 Markdown 迁移。

Phase 2 Task 编码前关闭 D-013。进入 AI 执行前关闭 D-016、D-017、D-005，并根据能力范围关闭 D-002、D-003、D-004 和 D-006。进入 Knowledge/Optimizer 前关闭 D-007、D-008 和 D-009。D-010 与 D-011 在公开发布或开放插件前关闭。
