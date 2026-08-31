# 待决策事项

这些问题在实现对应阶段前必须关闭。未关闭不代表 V1 文档无效，但不得在代码中静默选择。决策也按产品阶段排序：任务管理 MVP 不应被后续 AI、Git 或 Optimizer 决策阻塞。

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
- 恢复和密钥丢失策略。

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

## D-010 项目和组件命名

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
- Done 后重新打开的语义和指标处理；
- Archived 是否允许直接恢复；
- 父子任务的完成约束和阻塞循环检测策略。

建议用 20–30 个真实任务样本做状态迁移演练后决定。

## D-014 TUI 与 GUI 交付策略

需在同一代码库内选择：

- 同时交付薄客户端；
- 先交付 TUI，再复用契约实现 GUI；
- 先交付 GUI，再补齐 TUI。

无论顺序如何，两端必须共享 Command/Query/Event contract，不允许共享数据库文件但各自实现业务规则。

## D-015 Task Event 与同步策略

需要决定：

- event sequence 的范围是全局还是按 aggregate；
- outbox 保留期限与压缩方式；
- 客户端 cursor、snapshot 和重放协议；
- 是否在首版提供实时推送，还是先用可靠轮询；
- 更正和删除在不可变时间线中的表达方式。

## D-016 AI 接入边界

进入 AI 阶段前需要冻结：

- 首个 Runtime 与最小 Adapter contract；
- AI 作为 Owner、Worker 或两者都支持；
- Assignment 与 Task/Subtask 的边界；
- AI 提交 Review 所需最小证据；
- 哪些操作只需 task policy，哪些需要安全 capability 和可信批准。

## 建议优先级

任务管理编码前优先关闭：

1. D-013 Task 最小模型与生命周期；
2. D-014 TUI 与 GUI 交付策略；
3. D-001 技术栈；
4. D-015 Task Event 与同步策略；
5. D-012 Markdown 迁移。

进入 AI 执行前关闭 D-016、D-005，并根据能力范围关闭 D-002、D-003、D-004 和 D-006。进入需求/偏好优化前关闭 D-007、D-008 和 D-009。D-010 与 D-011 在公开发布或开放插件前关闭。
