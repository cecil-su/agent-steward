# 接续、证据与验收补充合同

状态：设计合同，待实现与验证。范围为阶段 A–C；不引入常驻 Supervisor、自动执行或新的一套任务状态。

本篇细化第 02/05/06 篇的证据、阻塞、Review 展示与接续验收要求，具体字段和行为以本篇为准。阶段范围仍由第 11 篇决定。

## 1. 最小 Evidence 语义（阶段 A）

Artifact 保存内容，EvidenceDescriptor 说明该内容支持什么结论。首期将 descriptor 保存为 Artifact 的不可变可选元数据，不引入独立 Evidence aggregate、生命周期、自动 verifier 或执行器。

| 字段 | 语义 |
|---|---|
| schemaVersion | descriptor schema 版本 |
| kind | test / command / git / human / statement |
| assertion | 验证的问题或记录的结论，不等同于系统判定 |
| outcome | passed / failed / unknown / not_run；只是本次观察结果，不是 Task 状态 |
| subjectRefs | 适用的 Task/criteria 或 Repository/Worktree/commit 引用；无代码的任务不强制 Git |
| observedAt / observationBasis | 观察时刻及 core_observed / externally_reported；外部报告时间不冒充核心采集时间 |
| decisiveResult | 最小可判断结果，如命令、退出码、测试范围、Git HEAD/dirty 状态 |
| origin | tool_result / system_observation / human_attestation / agent_claim / imported |
| sourceRef / producer | 可追溯来源和生产者，由可信入口派生或明确标为外部报告 |
| descriptorHash | 核心对排除 descriptorHash 本身后的规范化 descriptor 计算 hash，与 Artifact.contentHash 分离 |

约束：

- descriptor 与正文在 finalize 后不可原地改写；更正或复测创建新的 Artifact，保留旧引用。普通附件无 descriptor 时仍可读取，但不能展示为验证通过。
- core 根据采集入口分配 origin。AI/导入文件即使声明工具已通过，也只能得到 agent_claim/imported；附一段日志不自动升级来源。
- 阶段 A 可以由核心只读 Git 观察和用户明确见证提供事实。没有可信测试采集入口时，测试报告显示为外部声明，不能为了生成 tool_result 隐式执行任意命令。
- human_attestation 只证明用户明确见证了所述事实，不把其他人的历史陈述转换成 tool_result。用户的 Review accept 是独立验收动作。
- 缺少完整受测现场绑定时，applicability 显示 unknown；同一 HEAD 下 dirty 变化不能被视为相同受测现场。首版不为解决此问题强制引入 WorktreeSnapshot，允许诚实显示无法确认适用性。
- unknown/not_run 不转成 passed；退出码 0 也不代表满足全部 acceptanceCriteria。
- ReviewSubmissionEvidence 固定 contentHash 与 descriptorHash（无 descriptor 用 null）；两者都纳入 evidenceSetHash。来源和适用范围更改必须重新提交，不能只保证正文 hash 不变。
- Review 视图逐项显示来源、结果、适用范围与证据缺失；用户可以基于其明确见证作验收，但系统不伪称已自动验证。任何来源的 passed 都不能自动写 Done。
- 原始输出使用受控 ArtifactLink 读取；descriptor 不保存密钥或整段无限日志，权限和保留沿用 Submission 独立 evidence Link。

## 2. Review 交付摘要（CLI 阶段 A；薄界面阶段 B）

`review show` 从指定 Submission、证据和 Decision 生成可重建的 ReviewSummary。删除派生缓存不丢失事实；不新增 Handoff 表或第二套 delivery 状态机，Task.reviewSummary 也不作为可写权威字段。

| 区域 | 展示要求 |
|---|---|
| 本次提交 | taskId、submissionId、reviewCycle、submittedTaskVersion、criteria hash |
| 改动与产物 | 关联文件/报告、提交时可知的代码基线、固定证据引用；未知项明确标注 |
| 验证 | EvidenceDescriptor 的结果、来源、适用性与原始输出入口 |
| 交付事实 | 本地改动、commit、push 分别展示 observed / unknown / not_performed，带引用和观察时间 |
| 尚未完成 | 从已固定的 submission-owned statement 证据中展示未完成项、风险与限制；未提供时标为未说明，不用后来 Comment 改写提交摘要 |
| 用户决定 | pending / accepted / changes_requested / withdrawn 及对应操作者、时间 |

本地 commit 不能证明已 push；没有远端观察时显示 unknown，不能推断未推送。只有明确执行记录或用户声明未执行时才显示 not_performed，并保留来源。自动 PR/deploy 不属于本篇。

固定的提交事实与后来重新读取的当前 Git 现场分别展示；dirty 工作区只能给出有范围的观察，不能冒充原子完整快照。LLM 可辅助表述摘要，但版本、结果、引用、交付事实和用户决定由核心读取，不从生成文本反向写状态。

## 3. 结构化阻塞与需要用户处理的视图（阶段 A/B）

Task 内只维护一个当前 `blocker`，替代自由文本 blockedReason；不另建 Attention aggregate 或通知队列。字段为：

- kind：input_required / environment / dependency / verification_failed；
- reason：具体原因；
- requiredActorId 或 requiredRole：谁能解锁；人未确定时明确使用 role，不猜身份；
- requiredInput：需要提供的信息或完成的动作；
- resumeCondition：满足什么条件才可以继续；
- evidenceRefs：支撑阻塞判断的可选证据；
- createdAt/createdBy：服务端记录，历史变更保留在 TaskEvent。

例：reason 为配置格式尚未确定，requiredRole 为 user，requiredInput 为确认 JSON 或 TOML，resumeCondition 为用户选择已记录并确认下一步是实现对应解析器。只写“等待用户”不足以提交 blocker。

`task block` 在单事务内验证 Task version/权限、保存 blocker、进入 Blocked 并写事件；已处于 Blocked 时用同一命令和当前版本更新 blocker。首版从 In Progress 阻塞，`task unblock` 要求记录 resolution、验证当前 owner 并清空当前 blocker，回到 In Progress；历史原因与 resolution 留在事件中。修改当前 blocker 同样需要 Task expectedVersion。

满足条件的陈述是带来源的输入，不自动触发恢复。Agent 可以在授权范围请求 unblock，但不能代替需要用户作出的决定，也不能借 blocker 获得审批权限。

`task list --view needs-input` 是按当前读取权限从 Blocked 与 pending Review 派生的只读列表，每项显示原因、责任人和下一步；它不改变 Task 状态。待验收不另写 blocker；V1 不加入 ack/snooze/escalation 或跨来源去重框架。

## 4. Review 编辑与撤回（D-020 设计已冻结）

采用显式撤回，避免编辑后版本变化使验收和返工同时失效：

1. 同一 Task 最多一个 pending Submission，数据库唯一约束；submit 只从 In Progress 进入 Review。
2. Review 期间拒绝改变 Task 业务版本的普通编辑、owner/上下文绑定变更、阻塞、取消和归档，返回 REVIEW_LOCKED 并指向 withdraw。要修改这些内容，先撤回。
3. Comment 与 TaskCheckpoint 是独立的追加记录，各自生成事件但不递增 Task.currentVersion；写入仍比较期望 Task 版本。它们不会改变固定的 criteria、evidence 或 Submission，也不能通过正文冒充 Task 更新。
4. `review withdraw` 接受当前 expectedTaskVersion、expectedSubmissionVersion 和 reason；由当前 Task Owner 或有管理权限的用户调用。必须验证 Task 处于 Review、Submission 是该 Task 唯一 pending、当前 owner 存在，随后原子将 Submission 标为 withdrawn、Task 退回 In Progress、递增两个版本、保存 withdrawnBy/withdrawnAt/withdrawReason 与事件。它不创建 accepted/changes_requested Decision，不要求证据可读，也不要求 Task version 等于 submittedTaskVersion，因此异常版本漂移时仍可安全退出。
5. withdraw 后编辑并重新 submit 会创建更大的 reviewCycle。旧 Submission 及证据保留，历史摘要仍可读取。
6. accept/request-changes 使用第 05 篇的 Task/Submission 与固定 hash 检查。二者及 withdraw 竞争时最多一个事务成功；旧命令不得影响新 cycle。Done 始终需要用户显式 accept。
7. archived Task 不能通过 withdraw 绕过归档规则；先通过受控 restore 恢复可操作状态。常规流程禁止直接归档 Review 中的 Task。

这些是待实现的设计规则，不是数据库现状声明。

## 5. 三条可重复接续旅程

状态均为待实现、待执行。测试使用独立临时 SQLite 与 Git fixture、固定时钟和确定 ID，不调用真实 Provider，不修改用户仓库；测试产生的日志只包含非敏感样本。具体运行命令与测试路径在技术栈冻结后添加，不能用文档检查冒充旅程通过。

### J-01：记录 → 重启 → 接续（阶段 A/B）

前置：临时目录创建一个有 owner/下一步的 In Progress Task，登记只读 Repo/Worktree；准备可读证据。

步骤：更新进度，保存 Checkpoint，停止全部产品进程并清空客户端缓存；重新打开，用 `task context` 获取当前任务和最近 Checkpoint。

成功断言：taskId、owner、Task version、nextAction、Checkpoint ID 和证据引用与持久状态一致；保存 Checkpoint 和重启均不产生 Done，也不要求存在 Session。相同 checkpoint 幂等键重放只得到原记录，版本不额外增加。

失败分支：写入使用过期 Task version，拒绝且不创建 Checkpoint；证据无法读取时返回 partial/missingRefs，不丢失当前 Task，也不把缺失内容当作完整恢复。

结果证据：重启前后 JSON、Checkpoint 与事件 ID、幂等副作用计数、当前 Git 观察。真实试用另记录恢复耗时和需要补充解释的内容。

### J-02：Review → 修改 → 撤回 → 重提（阶段 A）

前置：In Progress Task 有固定 criteria 和一组带 descriptor 的 Artifact；submit 创建 cycle 1。

步骤：尝试直接修改 owner/criteria，断言 REVIEW_LOCKED 且版本不变；使用当前两个版本 withdraw，再修改 Task，submit 创建 cycle 2，用户验收。

成功断言：cycle 1 为 withdrawn，旧证据不变；cycle 2 使用新 Task/criteria/evidence hash；只有 cycle 2 的用户 accept 产生 Done，交付摘要能读取两个 cycle 各自事实。

失败分支：旧 cycle accept 被拒绝；并发 accept 与 withdraw 最多一个成功；证据 descriptor 或 content hash 不匹配不得 accept；agent_claim 的 passed 不显示成系统验证，也不自动关闭 Task。request-changes 分支应能退回 In Progress 并产生 Decision。

结果证据：每条 Command 的前后版本、Submission/Decision/withdraw 事件、证据集合 hash、冲突返回以及实际写入次数。

### J-03：Checkpoint → Git 现场变化或缺失 → 接续（阶段 B）

前置：Checkpoint 保存 Repo/Worktree 关联和带时间的 HEAD/dirty 观察。

步骤：分别从同一 baseline 分叉：切换临时分支、同 HEAD 修改文件、让登记路径 missing；查询 `task context`。

成功断言：稳定 Task/Repository identity 不被路径字符串猜测替换；分别显示 HEAD 变化、dirty 变化和 missing；当前事实与 Checkpoint 分列。dirty 检测不足以确认内容相同时显示 applicability unknown，不能宣称证据仍覆盖当前现场。

失败分支：Git 读取失败时返回 unavailable 与恢复动作，不创建替代 Repo，不执行 checkout/reset/clean、不删除目录，不拿旧 HEAD 填充为新观察。

结果证据：前后 Git 观察、TaskContextBinding、上下文差异、错误码和 fixture 文件完整性。单元/契约通过与真实隔日接续成功分别报告。

## 6. 有界恢复上下文（阶段 C）

`task context` 按权限装配：当前目标/criteria/owner/下一步与 blocker → 已确认限制/关键决策 → 最近 Checkpoint 差异 → 历史与证据引用。当前事实优先于摘要，生成文本不得覆盖它们。

- 首版控制本工具响应的 UTF-8 字节数：默认 32 KiB，用户可缩小，上限 128 KiB；不声称这是宿主完整 token 预算，也不推断剩余窗口。
- 响应包含 taskVersion、checkpointId、Git observedAt、budgetBytes、returnedBytes、truncated、omittedSections 与 continuationRefs。返回合法 JSON，不在序列化后直接截断字符串。
- 核心目标、criteria、当前权限范围/版本、blocker 或明确限制放不下时，返回 CONTEXT_BUDGET_TOO_SMALL 和最低需求提示，不能静默丢弃后继续执行。
- 可省略的历史只返回最小授权引用和展开方式；下一页重新鉴权。权限不足的条目不通过 omittedSections 泄露对象 ID 或内容。
- 引用展开时版本变化必须显式提示重新取 context；字节估计、摘要与缓存不成为状态权威。不采集完整会话，不实现自动 compaction/rollover。

阶段 C 验收补充：大历史下必需事实仍完整；截断与展开可读；版本变化触发刷新；越权来源不泄漏。默认预算是本地产品选择，日用结果可驱动显式调整，不照搬外部项目模型阈值。

## 7. CLI Skill 接入合同（阶段 C）

在 CLI 稳定后提供一个小型接续 Skill；本篇只定义未来交付要求，不安装 Skill，也不增加 Skill Runtime。先用现有会话与 CLI 验证价值，再评估 MCP 是否带来收益。

Skill 只约定：

1. 开始时明确 taskId，读取 context，核对当前版本、owner、scope 与 Git 差异；多个候选先选择。
2. 实质变化通过受控命令写 Task/Comment/blocker；发生版本冲突重新读取，不反复重放旧 Patch。
3. 中断前保存 TaskCheckpoint；未成功保存必须说明，不能把聊天摘要视为持久化成功。
4. 结果通过 EvidenceDescriptor 与 ArtifactLink 提交 Review，区分声明与观察；用户显式验收才 Done。
5. 重试相同语义写入复用幂等键；操作范围与授权由核心检查，Skill 文本不签发权限，不复制 token 到 Prompt 或可继承环境变量。

验收：两个现有会话接续同一真实任务，不启动受管 Agent；过期版本、保存失败和截断上下文被正确处理。CLI JSON/schema 是接口依据，Skill 随接口版本更新，不能通过解析人类输出维护另一套状态。

## 8. 接入能力与验收状态（阶段 C；控制能力按阶段 D 启用）

接入不能只用一个“支持/不支持”标签。CLI Skill、MCP 和后续 Runtime 分别记录以下维度，不建立 Provider Registry 或第二套任务状态：

| 维度 | 记录要求 |
|---|---|
| 能力覆盖 | 明确查询、写进度、保存 Checkpoint 等操作是否实现；阶段 C 不因已连接就声称可 spawn/resume/interrupt |
| 契约验证 | 代码/接口版本、测试范围、平台、时间与结果引用；仅模拟宿主时明确说明 |
| 当前环境 | 当前连接、授权 scope、必要入口及可访问数据是否已核对；检查带观察时间，无法核对为 unknown |
| 真实接续 | 是否实际由两个已有会话接续同一 Task，记录版本、场景、结果与限制；不能用安装成功或一条 ping 替代 |

验收记录是带来源的信息，环境检查是可失效的当前观察。历史通过不能覆盖当前连接失败；当前连接成功也不证明权限边界或恢复旅程通过。产品只展示本地有依据的结果，不继承第三方 README 的支持声明。

身份、权限、状态机与幂等约束由核心执行。环境检查不得自动安装客户端、修改宿主信任、签发新 scope 或启动模型回合。只读查询和基础 CLI 在 AI 不可用时仍可使用。

## 9. 只读诊断与首次接续（阶段 A–C）

### 诊断输出

诊断复用当前 Query 和授权，不新增诊断状态表或后台修复器。只读 `doctor` 候选入口与薄界面显示同一结果，至少包括问题类别、当前观察、观察时间、可执行的下一步及其是否需要写操作。没有证据时明确 unknown，不把结果未确认显示为失败。

| 情形 | 恢复指引 | 禁止的自动动作 |
|---|---|---|
| 数据无法打开或格式不符 | 显示已授权可见的路径/格式诊断，核对程序版本与备份 | 删除、重建或隐式迁移 |
| Task 上下文有多个候选 | 返回权限范围内候选并要求显式选择 | 按最近更新时间猜写入目标 |
| 授权过期或 scope 不足 | 返回受控重新连接步骤，重新读取当前 Task | 恢复旧授权或在提示中附带 secret |
| Checkpoint 引用缺失 | 保留当前 Task，标明可见范围内 missingRefs/partial 并给出核对动作 | 把旧摘要当完整恢复结果 |
| 写请求结果未知 | 使用原幂等键及相同语义请求按 Receipt 合同核对；版本和 generation 不得为重试擅自更新 | 换新 key 重做同一未知操作 |
| Git 现场改变或缺失 | 重新读取当前 Git 并区分 Checkpoint 观察 | checkout/reset/clean 或重建替代 Repo |

诊断不得绕过读取权限泄露对象 ID、路径或凭据；写入修复仍需显式受控 Command。支持“未执行”结论的证据不足时保持结果未知。恢复旧备份后的 generation 校验仍受 D-021 约束，不能以本节替代该未决合同。

### 首次可重复接续

首次体验采用一个可丢弃的本地任务，不要求先注册多个 Workspace、连接 Supervisor 或安装多个客户端：

1. 从当前目录创建 Task，确认默认 Workspace、目标、owner、criteria 和下一步。
2. 保存进展与 TaskCheckpoint，停止产品进程，清空客户端缓存后重开。
3. 读取 task context，说明当前下一步与缺失项；代码任务另读取当前 Git。
4. 固定至少一项有明确来源的产物/声明并提交 Review，用户根据可见来源显式 accept 或 request-changes；没有真实检查时不得展示为系统验证通过。
5. 阶段 C 再由第二个已有 AI 会话重复读取与接续，验证原授权过期、版本冲突和 Checkpoint 保存失败的处理。

阶段 A/B 复用 J-01～J-03，不为首次体验新建业务 API 或另一套完成条件。记录恢复耗时、需要重复解释的内容及失败点；测试 fixture 与真实使用分开。需要检查工具结果时只能用已授权的可信采集入口，不能为了完成向导隐式执行任意命令。

## 10. 借鉴来源与采用边界

以下基于 2026-09-07 调研读取的 Xuanwu main 文档与部分源码，2026-09-08 完成 V1 文档整理；链接为可变上游分支，实施前需重新核对。它们是设计参考，不是已在本项目运行验证的依赖。仅借鉴语义和验收方式，不复制其调度、兼容迁移或权限实现。

| 来源 | 借鉴点 | 本项目取舍 |
|---|---|---|
| [Evidence 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0027-evidence-domain-contract.md) 与 [代码](https://github.com/williamnie/xuanwu/blob/main/backend-ts/src/domain/evidence/contracts.ts) | 来源、决定性结果与正文引用分开 | Artifact 的不可变 descriptor，不新建状态机 |
| [Handoff 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0036-handoff-delivery-contract.md) | 交付摘要由事实投影 | ReviewSummary 派生查询，无独立交付表 |
| [Attention 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0061-unified-attention-model.md) | 明确需要谁处理什么 | Task.blocker 与只读 needs-input 视图 |
| [Golden Journey 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0003-golden-journey-contracts.md) | 成功、失败分支及可复核结果 | 三条任务接续旅程，另做真实试用 |
| [上下文预算](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0092-im-context-budget-and-session-rollover.md) | 有界装配、引用与按需展开 | 只限定 context 响应，不控制宿主窗口 |
| [CLI Skill](https://github.com/williamnie/xuanwu/blob/main/skills/xuanwu/SKILL.md) | 现有会话使用 CLI 接入 | 保持本项目 capability 传递与人工验收约束 |
| [项目支持范围](https://github.com/williamnie/xuanwu#provider-support) | 能力代码与真实验收分层 | 采用第 8 节的独立维度，不继承第三方验证结论 |
| [首次交付](https://github.com/williamnie/xuanwu/blob/main/docs/first-delivery.md) | 完整旅程与具体恢复步骤 | 第 9 节复用 Task/Checkpoint/Review，不引入 Supervisor 引导 |
| [重启恢复](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0069-restart-recovery-invariants.md) | 持久状态与回执优先，不盲目重放 | 复用 Receipt，阶段 D 外部副作用另按第 08 篇与 D-022 冻结 |

本轮只完成设计整理。J-01～J-03、首次接续与接入验收均待实现和执行；D-020 为已冻结设计，D-021/D-022/D-024 等剩余决策按第 12 篇推进。V0 的 CLI、Hook、GUI 及其测试不计入这些 V1 结果。
