# V1 实施路线

V1 采用可验证的垂直切片推进。排序原则是：先证明任务管理价值，再证明 AI 增强价值，最后证明长期优化价值。

## Phase 0：任务管理基线与技术实验

- 冻结 Task 最小字段、状态机、关系和验收语义；
- 选择技术栈、SQLite migration 方案和本地进程边界；
- 确认 TUI/GUI 交付策略与共享 Command/Query/Event 契约；
- 验证事务写入 Task + Task Event、版本冲突和事件续传；
- 定义最小数据备份、导出和恢复方案；
- 用低保真原型验证 Inbox、Next、Blocked、Review 四条高频路径。

退出条件：不依赖 AI 的任务闭环、权威数据和客户端边界没有关键歧义。

## Phase 1：本地任务内核

- `taskd` skeleton、SQLite schema 和 migration；
- Project、Task、Subtask、Actor、Ownership 与 TaskRelation；
- Inbox/Ready/In Progress/Blocked/Review/Done 状态机；
- Command、Query、expectedVersion、幂等和 event outbox；
- 评论、Artifact 元数据、ReviewDecision 和时间线；
- backup/export/import 基础能力。

退出条件：契约测试能完整跑通捕获、分流、执行、阻塞、验收、返工和归档。

## Phase 2：可日用的 TUI 与 GUI

- TUI：快速捕获、键盘导航、过滤、批量分流和状态推进；
- GUI：项目/列表/看板、依赖、任务详情、时间线和 Review；
- snapshot + event cursor 同步、断线恢复和版本冲突界面；
- Inbox、Next、Owner、Blocked、Review 和 Saved View；
- 搜索、排序、提醒、空状态、错误恢复和可访问性；
- 真实个人项目 dogfooding 与数据迁移测试。

退出条件：用户可以只靠该产品持续管理真实项目，且 TUI/GUI 不出现状态分叉。

## Phase 3：任务流程体验完善

- recurring/template、批量编辑和快捷命令；
- blocker aging、无 owner/无下一步检测和 Review 队列；
- 更好的依赖可视化、活动摘要和通知策略；
- Markdown/JSON 导出与外部链接；
- 性能、备份、恢复、崩溃一致性和跨平台打包。

退出条件：核心任务指标稳定，常用操作无需依赖 AI 补足产品缺口。

## Phase 4：AI 执行增强

- 通用 AI Actor、Assignment、Session、Invocation 和 AgentRun；
- `steward-mcp` 与一个首选 Runtime Adapter；
- ContextBrief、Artifact、阻塞/恢复和 Review handoff；
- capability、owner epoch、最小审批与审计；
- AI 完成不直接 Done 的端到端验证；
- 需要时加入 Git inspect 与受控本地操作。

退出条件：人类和 AI 使用同一 Task，AI 能提高执行效率而不破坏状态与验收权威。

## Phase 5：需求、偏好与流程优化

- Requirement、Decision、Preference 与 Feedback；
- 来源、作用域、置信度、确认和 supersede 流程；
- 重复澄清、返工、阻塞和上下文浪费的确定性指标；
- Prompt、流程、模板和 Skill 候选；
- 用户确认、replay/shadow/canary 和回滚；
- ContextBrief 按任务选择已确认信息，避免全量注入。

退出条件：候选不自动生效，并能用任务历史证明至少一类效率损耗下降。

## Phase 6：高级集成与强化安全

- 更多 Runtime Adapter 与 conformance suite；
- 完整 Session Store、本地加密和多宿主 importer；
- Git commit/merge/push 的 Plan → Approve → Execute → Verify；
- credential broker、Agent 隔离和 Hardened 模式；
- 插件、Policy Pack、签名与兼容策略。

退出条件：扩展能力不会绕过 taskd，敏感凭据和高风险操作满足对应威胁模型。

## 发布建议

- `0.1.x`：本地任务内核 + 可日用 TUI/GUI。
- `0.2.x`：一个 AI Runtime 的完整任务执行闭环。
- `0.3.x`：需求、偏好和优化候选。
- `0.4.x`：更多 Runtime、Git 治理和 Hardened 能力。
- `1.0.0`：任务 schema、客户端契约、迁移和扩展 API 稳定。

每个版本都必须保留上一层独立价值：AI 不可用时 `0.2+` 仍是完整任务管理器，Optimizer 不可用时 `0.3+` 仍能正常管理和执行任务。
