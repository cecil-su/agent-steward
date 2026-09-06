# V1 需求与验收

本文定义阶段 A–C 的主线；阶段 D 是可选执行增强，X 是后续探索。技术契约按能力启用，不因已有模型定义而提前实现。

## 阶段 A：任务开始、记录与交付

### FR-001 当前目录开始任务

- CLI 能创建、查看和选择任务，提供稳定 JSON 输出。
- Task 包含目标、状态、优先级、owner、下一步和验收标准；Task.workspaceId 必填。
- 系统可以自动建立默认 Workspace，并识别当前 Repository/Worktree。显式选择已有 Workspace 时必须尊重选择。
- 用户不必先扫描全部目录或完成 Registry onboarding；非代码任务可不绑定 Repository。
- 同一 Git common dir 下的 linked Worktree 不得误识别为不同 Repository；无法确认身份时显式返回冲突，不猜测绑定。

### FR-002 进度与恢复记录

- 能更新下一步、记录阻塞、关键决策及来源，并关联产物。
- 提供 TaskCheckpoint：引用 Task version、当前 owner、证据、Git 观察与时间；说明已完成内容、未决问题和下一步。
- Checkpoint 可以标记 partial，列出缺失引用；中断不自动完成或关闭 Task。
- TaskCheckpoint 不要求 Assignment、Session、ContextWindow 或 Runtime 存在。

### FR-003 人工验收

- 最小生命周期支持 Inbox、Ready、In Progress、Blocked、Review、Done、Cancelled；归档独立于生命周期。
- owner 和下一步用于明确责任；普通人类任务不需要 AI grant 才能成立。
- 提交 Review 时固定验收条件、Task 版本和证据；用户可以验收或要求返工。
- 同一用户可以推进自己的任务并显式验收；AI 不得以完成声明自动写 Done。
- Review 期间编辑、撤回及重新提交的规则必须在编码前关闭，避免版本变化后无法返工。

### FR-004 最小代码上下文与本地恢复

- 只读获取 Repository/Worktree 的规范路径、Branch、HEAD、dirty/missing 状态，并显示观察时间。
- TaskContextBinding 是持久关联；Git 命令读取的是当前现场，旧 Checkpoint 不代表当前 HEAD。
- 登记、刷新、解除关联均不修改 Git 或删除目录；全盘发现、移动自动跟踪和多 Workspace 管理可延后。
- SQLite 保存当前状态、版本、幂等记录和领域事件。首期采用明确停写的维护备份与隔离验证恢复。
- 如引入 Blob 证据，发布与引用必须满足 publish-before-reference；不以简化范围为由允许悬空引用。

## 阶段 B：日常接续与证据复核

### FR-101 恢复工作

- 从任务列表或当前目录定位任务，多个候选由用户选择，禁止猜测写入目标。
- 恢复视图组合当前 Task、最近 Checkpoint、关键决策、证据和重新读取的 Git 状态。
- 显示版本、owner 或代码现场与 Checkpoint 的差异；过期引用不静默进入执行上下文。
- 支持重启、隔日接续、Worktree missing 和证据不可读等明确恢复路径。

### FR-102 日用界面与简单复用

- CLI 保持完整任务闭环，选择一个薄 TUI 或 GUI 覆盖任务列表、详情、恢复和验收。
- 第二客户端不是阶段退出条件；所有客户端共享 Command/Query 和权限规则。
- 项目约束、关键决策和操作说明可以作为带来源的简单记录链接到任务；人工选择复用，不自动推断长期事实。
- 子任务、依赖、看板、SavedView、通知和模板仅在主线稳定且有真实使用证据后安排。

## 阶段 C：接入现有 AI 会话

### FR-201 读取与回写同一任务

- 现有 AI 会话通过 CLI/MCP 获取精简恢复上下文，包含目标、当前状态、下一步、约束和证据引用。
- AI 使用服务端绑定身份和限定 Task scope 写进度、阻塞、TaskCheckpoint 与完成候选；遵守版本与幂等检查。
- 不要求 spawn/resume/close、AgentRun、完整 Session importer 或宿主 context transition。
- 新会话重新读取当前任务与权限，不盲目恢复旧授权或旧摘要。
- 仅保存交接所需记录；完整 Prompt、工具输出和会话正文采集须另行选择范围。

## 可选阶段 D 与探索 X

| 能力 | 进入条件 | 保留约束 |
|---|---|---|
| D：控制一个 Runtime | C 的接续流程已日用，用户反复需要手动启动或恢复 Agent | RuntimeOperation、固定目标、幂等、未知结果 reconcile |
| 后续 Git 写入 | 有明确执行需求且可信批准与现场校验已闭合 | Plan → Approve → Execute → Verify |
| X：结构化业务事实与漂移 | 简单约束和决策已被频繁复用，人工维护成本有证据 | 事实版本绑定、候选确认、快照证据 |
| X：Optimizer | 已有可比较的重复返工和上下文损失记录 | 最小采集、独立授权、候选评估与回滚 |

这些能力不阻塞 V1 发布，不能成为阶段 A–C 的依赖。

## 共用质量要求

- 状态、版本、CommandReceipt 和对应 DomainEvent 在同一事务提交；幂等重放仍检查当前读取权限。
- 更新使用 expectedVersion，跨对象变更使用明确的各对象前置版本；冲突不静默覆盖。
- 多客户端启用事件同步时，snapshot 与 watermark 来自同一读取事务；restore 必须使旧请求与同步上下文失效，具体 generation 契约列入待决策事项。
- 默认离线可用、无遥测；读取或输出敏感内容按 scope 控制。Standard 不承诺抵御同 OS 身份的恶意 Agent。
- 核心逻辑与客户端分离；常驻服务、在线备份和多 Runtime 不属于无条件架构要求。

## 用户结果验收

阶段 A 开始前选取至少 5 个真实任务，记录原有接续方式、耗时与重复解释情况；首轮试用可采用以下目标，评估后显式调整：

| 场景 | 验收方式 |
|---|---|
| 第一次使用 | 从当前目录完成建任务和保存下一步，无需先进入 Workspace 管理页 |
| 隔日恢复 | 5 个样本中至少 4 个能在 2 分钟内确认当前目标、进度、证据和下一步，无需重读完整聊天 |
| 换会话 | 新会话仅使用产品提供的上下文即可说明下一步及关键限制；用户记录缺失信息和重复解释次数 |
| 验收 | 用户能定位证据并明确接受或返工；证据缺失、版本变化必须可见 |
| 故障恢复 | 重启、版本冲突、missing Worktree 和备份恢复不静默丢失已确认状态或误写其他任务 |

这些是试用目标，不是已完成的测试结果。是否进入下一阶段由契约验证和真实使用反馈共同决定。
