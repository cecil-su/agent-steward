# 待决策事项

决策按阶段 A–C 主线关闭。D 是可选执行增强，X 是探索；后期问题不得阻塞首期任务接续。本文保留原 D 编号便于追溯，但重排适用阶段。

## 阶段 A 编码前

### D-001 技术栈、平台与核心部署

选择技术栈、SQLite 方案和首个验证平台。先验证一个平台的真实任务路径，再评估跨平台分发；不把三平台全部验证作为首轮试用前提。嵌入式核心与服务部署的选择见 D-019。

### D-012 既有数据接续

核查是否需要导入实际 V0 Task/Checkpoint。若需要，定义一次性导入、来源 ID、字段转换和验证；没有实际数据迁移需求时不预建 importer。Markdown 仅作为导入来源或只读导出，不能成为长期双向权威。

### D-013 Task 最小生命周期

冻结创建、开始、阻塞、恢复、提交验收、返工、取消、重开及归档的合法转换和 owner/nextAction 条件。Done 只由用户显式 accept 产生，重新打开进入 In Progress。

首版不要求父子任务、复杂依赖、自动选任务或周期模板。Review 期间编辑与撤回问题见 D-020，不能留待实现者猜测。

### D-015 事件与恢复后同步

状态、Receipt 与对应事件同事务。启用增量同步时使用独立于 aggregateVersion 的 streamPosition 和同事务 snapshot + watermark，未知 schema 显式拒绝或 upcast，过期 cursor 强制重新获取 snapshot。

首版可以按需查询，不要求实时推送。restore generation 对 cursor、请求、连接和幂等记录的影响见 D-021；事件保留、客户端缓存与审计删除策略按实际数据范围冻结。

### D-018 默认 Workspace 与最小 Repository identity

- 从当前目录开始任务时，如何选择或自动建立默认 Workspace；显式指定与自动发现发生冲突时如何提示。
- workspaceId 是持久归属，rootPath 是发现范围，不授予文件删除权限；不要求首期实现多 Workspace 管理界面。
- Repository identity 组合 canonical real path、Git common dir 与可选 remote；linked Worktree 必须归为同一 Repository，独立 clone 不能仅因相同 remote 被合并。
- 先冻结首个支持平台上的大小写、symlink、路径缺失与重复登记；nested Repo、submodule、bare Repo 未支持时明确提示，不静默误识别。
- Task 多个候选上下文、missing/unlinked 和重新绑定的规则；解绑只改关联，不改用户目录。

### D-019 Application Service 与维护备份

统一业务入口是固定边界。选择本地嵌入式核心或 taskd 服务，并确保多进程写入仍使用同一事务、身份和策略实现。

首期采用显式维护停写备份，冻结如何排除全部 writer/worker、验证 SQLite 与可选 Blob、发布和恢复整套数据。在线备份的 operation/pin/generation 协议仅在实际需要在线服务时扩展；恢复的跨代失效问题仍必须先解决。

### D-020 Review 编辑与撤回

未解决：accept/request-changes 要求 current Task version 等于 submittedTaskVersion；更换 owner 或编辑 Task 后可能同时无法验收和返工。

编码前选择并定义：禁止哪些 Review 期间编辑，哪些编辑原子撤回 Submission，以及显式 withdraw 如何使用当前版本退回 In Progress。旧 Submission 与证据保留，新 submit 创建新 cycle。仅有 withdrawn 枚举不算完整契约。

### D-021 Restore generation

未解决：恢复旧 SQLite 会回退对象版本并丢失备份之后的 Receipt，旧请求可能再次满足 expectedVersion。

冻结新 data generation 在 Command、幂等 scope、Query snapshot、event cursor 和连接中的传播与校验。恢复后必须拒绝旧 generation 的写入并强制重建同步上下文；只撤销短期 capability 不足以覆盖人类 CLI。覆盖“备份后执行成功、恢复、旧请求重试”的验收场景。

### D-024 TaskCheckpoint 与最小证据

冻结不依赖 Assignment/Session/ContextWindow 的 TaskCheckpoint schema：Task/version、当前 owner 引用、进度说明、唯一下一步、未决问题、决策与证据引用、Git 观察时间，以及 partial/missingRefs。

确定首版证据保存 SQLite 小内容还是 Artifact Blob，以及不可变、大小、读取权限、保留和维护备份要求。普通文件路径可作为定位线索，不能把会变化或丢失的文件路径宣称为已固定的 Review evidence。

## 阶段 B 前

### D-014 首个薄界面

CLI 先覆盖完整闭环。根据实际任务样本选择一个 TUI 或 GUI 用于列表、恢复、详情和验收；第二客户端无承诺日期。共享 Command/Query/权限规则，展示可不同。实时 Event Stream 是否需要由交互需求决定。

### D-009 已纳管数据的保留与删除

在首批证据进入持久存储前先冻结最小保留/删除规则；阶段 B 再根据日用规模调整容量与历史查询。删除必须处理引用、索引和最小审计事实，不能破坏保留中的 Review evidence。完整 Session/embedding 策略留待对应能力启用。

## 阶段 C 前

### D-016 现有 AI 会话接入

冻结受控 CLI/MCP 的身份建立、Task scope、owner 变更失效、上下文查询和进度/Checkpoint/完成候选写入。AI 不能自行声明 Human 身份或自动关闭任务。

这个阶段不要求 Runtime Adapter、Assignment、spawn/resume 或 Session importer。若需要会话关联，只保存可选外部 session reference，不将其变成 Task 的父对象。

### D-002 / D-003 安全模式与用户确认

阶段 C 明确 Standard 的协作完整性承诺、Human 与 AI 连接建立方式和验收入口。若威胁模型要求阻止同 OS 身份 Agent 绕过入口，必须先实现独立身份/沙箱等 Hardened 边界。

可信高风险批准在启用 Git 写入或其他敏感副作用前另外冻结；不能把普通同身份 CLI 命令宣称为对抗性批准渠道。

### D-006 加密与恢复材料

在实际保存敏感 Blob 前冻结加密、密钥保护和恢复方案；阶段 C 明确最小交接数据范围。可移植加密备份需口令/恢复密钥或可用外部 provider 包装的 key envelope，不保存明文密钥，也不把设备专属 Keychain 引用当作跨设备恢复材料。

## 可选阶段 D 与后续集成前

### D-005 一个 Runtime 的选择

先验证阶段 C 的手工启动/恢复成本，再从可用宿主中选择一个 Runtime。Herdr、Pi、Claude、Codex 都是候选，不要求 Herdr 加 Native Host 双实现。

### D-017 宿主上下文与工作记忆

所选 Runtime 需要时才定义 ContextWindow、transition mode、History、WorkingNote 和 ContextCheckpoint。TaskCheckpoint 仍负责不依赖宿主的任务接续。不得把读取完整会话、控制 compaction 或存储每条工具输出当作默认前提。

### D-022 Runtime 重试目标稳定性

未解决：先解析 active Run 再计算 requestHash，会让 close 成功后的重试找不到 Run，或在新 Run 出现后解析到不同目标。

冻结首次请求目标与重试匹配顺序：命中 Receipt 使用持久化 Run/Handle 并重验读取权限；只有首次请求解析 active Run，或命令显式绑定 agentRunId。必须覆盖响应丢失后 close 重试及 Run 替换测试。

### D-004 Git 写入策略

只读上下文识别不依赖本决策。启用 commit/merge/push 前冻结策略、可信批准、Git 现场变化检查和外部副作用恢复。force push/reset hard/clean 不属于 V1 主线。

## 探索 X 与公开扩展前

### D-007 / D-008 完整会话采集与分析模型

先说明具体用途、数据范围、收益验证与保留策略，再决定 importer、全文索引或 embedding。默认不采集完整会话；任何远程分析需要单独、可见、可撤销的授权。

### D-023 事实版本与实现观察

未解决：MappingObservation/DriftFinding 未明确绑定所评估 FactRevision，事实更新后旧 current 可能被误用于新规则。

进入知识实现前冻结 factRevisionId 绑定与重新评估规则；若 freshness 仅表示实现位置存在，必须与业务符合性分开展示。简单决策记录不依赖此模型。

### D-010 / D-011 名称、发布与插件

公开发布前冻结名称和 License；只有开放插件时才定义插件权限、签名与 API 兼容规则。不为尚未需要的多 Runtime/Policy Pack 提前建设平台框架。

## 决策顺序

阶段 A 优先 D-001、D-012、D-013、D-018–D-021、D-024 和 D-015 的最小子集；首次保存数据时关闭 D-009/D-006 中适用部分。B 关闭 D-014，C 关闭 D-016 和适用安全项。D/X 决策仅在对应能力获准进入时关闭。
