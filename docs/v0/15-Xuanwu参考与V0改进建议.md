# Xuanwu 参考与 V0 改进建议

日期：2026-09-08。状态：参考分析与待验证建议，不是功能完成记录。

本次对照基线为 `codex/v0-taskctl` 的 `1783c5c` 与当前工作树；本次只修改文档，不为工作树内已有 UI/CI 改动提供验收背书。V0 与 V1 是独立方案，本篇不采用 V1 的 ReviewSubmission、EvidenceDescriptor、Owner 或阶段 A–D 模型。

## 1. 定位与取舍

[Xuanwu](https://github.com/williamnie/xuanwu) 是常驻 AI 工程控制平台，围绕 Work、Run/Attempt、Evidence、Handoff、Attention 和 Automation 组织执行、监督与恢复。其支持标签区分实际验收程度，不能把存在适配代码等同于可用。这里依据上游 main 文档和此前读取的部分源码分析，未安装运行玄武，也未独立复测其支持声明；链接随 main 演进，实施时应重新核对。

V0 管理任务上下文、执行 Session、Checkpoint、History 和轻量 Worktree，并以 M4 Hook 收集被动观察、M5 提供本地界面。最有价值的参考是交接信息质量、验收方式与恢复纪律，不是复制常驻调度平台。

## 2. 对照当前实现

| 参考点 | V0 已有基础 | 值得补充的最小做法 | 状态 |
| --- | --- | --- | --- |
| 验证有来源和范围 | Checkpoint.completed/risks、Task notes、实时 Git 观察 | 记录实际检查、受测版本/现场、结果和未验证项；注明是人工/Agent 报告还是当前工具观察 | 写作约定，可立即使用；结构化采集未实现 |
| 可审查交付 | task context、Checkpoint、History、GUI | 复用这些入口展示已完成、待完成、验证、交付位置和下一步 | 内容约定可用；专用摘要展示待评估 |
| 明确用户需要做什么 | blockReason、blockRecovery、nextStep | 原因写清缺什么；恢复说明写清谁处理、做什么、如何确认可继续 | 复用已有字段，不新增 Attention |
| 能力与验收分离 | Codex/pi 原生入口、契约测试与接入文档 | 分别报告代码、契约测试、本机配置、真实会话验收；带版本、日期、平台 | 报告约定；自动诊断待实现 |
| 结果未知不盲目重试 | GUI uncertain/CAS 反馈、Worktree PARTIAL_EXTERNAL_STATE 与 adopt/detach | 补齐按错误类别组织的只读核对步骤和回归场景 | 核心机制已有；补充验收待执行 |
| 首次完整接续 | 创建、claim、checkpoint、resume、context 与 GUI 测试 | 完成一次停止进程、重新打开、换 Session 接续的真实旅程 | 待专项执行并记录结果 |

实现依据：`crates/core/src/lib.rs` 的 CheckpointInput/TaskView、`crates/application/src/hooks.rs`、`crates/server/web/app.js`、`integrations/README.md`。Checkpoint 是报告，不是系统自动证明；Hook 的 tool_result 仅是事件类型，不表示工具成功、测试通过或满足验收条件。

## 3. 可立即采用的记录约定

不增加 DTO 字段或修改 SQLite。继续使用既有 `summary/completed/decisions/pending/nextStep/risks`，可按以下内容填写：

- summary：本次解决的问题及当前交付范围。
- completed：实际完成事项；检查记录写明命令/检查范围、结果、时间及来源。若只有他人报告，应明确标为报告。
- decisions：关键取舍与限制依据。
- pending：尚未执行的真实客户端、平台或场景验收。
- nextStep：下一位接手者可以执行的具体动作。
- risks：受测现场与当前现场可能不同、外部结果未知等限制。

交付信息可放在 summary/completed/pending 中，分别说明本地改动、commit、push 的事实及引用。只看到本地 commit 不能证明远端已更新；没有远端观察时记为未确认。同一 HEAD 下 dirty 内容变化也不能证明旧测试仍适用。不要保存凭据或无限日志；外部文件引用须说明位置与可用性，不承诺存在独立 Artifact Store。

阻塞例：blockReason 写“pi 会话尚未显式绑定，无法确认观测归属”；blockRecovery 写“操作者取得实际会话 ID，核对 Task 最新版本并执行 session bind，再确认 hook list 出现该会话观察”。V0 不根据这段文字自动恢复或关闭任务。

## 4. Codex/pi 验收报告与诊断建议

每个适配器分别记录以下四项，不能用单一“支持”标签替代：

| 项目 | 要回答的问题 |
| --- | --- |
| 代码覆盖 | 哪些宿主事件已映射，哪些明确不支持？ |
| 契约测试 | 哪个提交、测试命令、平台和日期通过了哪些负载/错误分支？ |
| 本机准备 | 二进制、必要配置、宿主信任和显式 Session 绑定是否已核对？无法检查时标未确认 |
| 真实会话 | 是否由实际客户端产生事件并入库，是否验证切换会话、失败回退及 Task 状态不变？ |

初始状态应从实际记录填写，不能沿用历史测试数量或客户端版本宣称当前全部通过。原生独立重复投递不保证去重；内部重试复用同一 ID/时间。原生 occurredAt 为适配器观察时间，不是可靠的宿主原始时间。

后续若扩展 doctor，先做只读检查与恢复指引，不自动安装、绑定、信任 Hook 或修改客户端配置。不要求增加 Provider Registry。缺失二进制、未绑定、宿主事件不支持和数据库错误应分开报告；输出遵守现有脱敏与权限边界。

## 5. 有针对性的接续与故障验收

下列为补充验收计划，不表示已执行；复用现有临时数据库、Git 与浏览器测试基础，不重建通用测试框架。

| 旅程 | 步骤 | 必须验证 |
| --- | --- | --- |
| V0-X1 重启接续 | Session A 创建/领取、更新并保存 Checkpoint；停止产品进程；重开并用新的 Session B 显式 resume | Task 意图、最近 Checkpoint 和 continuedFrom 可读；使用最新 CAS；不因重启或 Hook closed 自动关闭 Task |
| V0-X2 现场变化 | Checkpoint 后分别更改 HEAD、同 HEAD 修改文件、让 Worktree 路径缺失，再读取 context/status | 区分旧记录与实时 Git；缺失明确报错，不用数据库路径冒充存在，不执行自动 reset/clean |
| V0-X3 写入结果未知 | CLI/HTTP 写成功后丢弃响应，再刷新核对；对 Worktree 部分完成保留现场并调查 | 不以超时判失败；不自动重放 create/resume 或 Git 写入；仅明确复核后操作，必要时显式 adopt/detach |
| V0-X4 原生 Hook | 用真实 Codex/pi 产生事件，再切换外部会话并制造接收器失败 | 正文不入库、错误会话不串写、失败不控制宿主、Task version/状态不由观察改变 |
| V0-X5 阻塞交接 | 记录原因与恢复步骤，另一会话读取并按权限处理 | 接手者能说清缺什么、谁处理、下一步；不能因收到消息或 idle 自动 unblock/close |

真实关闭测试必须由操作者明确选择，不能由 Agent 根据测试通过自行关闭。关闭后的 Task 不重开；需要后续工作时按既有合同新建任务。

每次验收保存代码版本、客户端/平台版本、测试范围、结果和限制。确定性 fixture、浏览器流程与真实客户端试用分别报告。现有测试覆盖的部分直接复用，只有缺口才补测试。

## 6. 优先级与不采用项

1. 优先：执行 V0-X1/X4，补齐真实接续和 Codex/pi 验收记录；日常 Checkpoint 采用第 3 节格式。
2. 按实际问题补充：只读诊断、交付信息展示和 V0-X2/X3/X5 的缺失回归分支。
3. 单独评估：若未来明确要求产品控制 Agent 或远端副作用，再讨论固定操作目标、意图/回执与恢复协议。V0 当前不能声称具备通用 exactly-once 或持久化 Operation。

本次不引入 Supervisor、自动任务拆分、Run/Attempt 表、多 Provider 调度、IM、通知队列、独立 Evidence/Handoff/Attention 状态机、自动 commit/push 或自动验收。也不以本次参考分析修改现有鉴权、数据库迁移或独立 UI 发布合同。

## 7. 来源与阅读顺序

- [项目介绍与 Provider 支持范围](https://github.com/williamnie/xuanwu)：参考能力与实际验收分开报告，不能继承其支持结论。
- [Evidence 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0027-evidence-domain-contract.md)与[部分源码](https://github.com/williamnie/xuanwu/blob/main/backend-ts/src/domain/evidence/contracts.ts)：借鉴来源与事实边界，V0 先采用记录约定。
- [重启恢复规则](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0069-restart-recovery-invariants.md)：借鉴先确认持久状态与外部结果、拒绝盲目重放；不照搬其恢复调度器。
- [首次交付与失败恢复](https://github.com/williamnie/xuanwu/blob/main/docs/first-delivery.md)：借鉴一条可完成的用户旅程及具体修复步骤，V0 不增加 Supervisor onboarding。
- [Run/Attempt 合同](https://github.com/williamnie/xuanwu/blob/main/docs/architecture/xuanwu/0020-run-attempt-lifecycle-contract.md)：仅作未来执行控制调研，不映射为 V0 Session 新状态。
