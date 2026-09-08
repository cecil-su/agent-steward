# Agent Steward V1 设计文档

状态：**以任务接续与交付为主线的设计基线草案**

产品承诺：随时知道一个任务做到哪里、依据是什么、下一步是什么，并能让另一个会话接着做。

## 阅读与契约优先级

先读[产品定位](01-product-positioning.md)、[需求与验收](02-requirements.md)和[实施路线](11-roadmap.md)，再读[核心工作流](14-core-workflow.md)、[总体架构](03-architecture.md)与[待决策事项](12-open-decisions.md)。

01、02、03、11、12 定义当前范围和阶段门槛。04–10 保存按能力启用的技术契约，不代表所有实体、组件和协议都要在首版实现。13–18 展示当前主线；19–20 仅描述后续知识探索；[第 21 篇](21-continuity-and-review-contracts.md)细化接续、证据、Review 与旅程验收。已有 Codex/Pi 评审属于历史记录，其旧范围建议不覆盖当前基线。

## 已确认方向

- 第一条用户路径是当前目录建任务、记录进度与证据、保存 Checkpoint、恢复工作、人工验收。
- Workspace 是内部归属边界，可以自动创建默认 Workspace；用户不必先完成 Registry onboarding。
- Repository/Worktree 身份识别随任务交付，Git 读取必须区分登记信息与当前现场；不把 Registry 单独作为首个产品版本。
- CLI 优先覆盖任务闭环；选一个薄界面辅助浏览和验收，另一客户端按需要补齐。所有入口复用同一 Application Service。
- SQLite 保存领域状态；状态变更、版本、幂等记录和对应事件在同一事务提交。
- 首期采用明确停写的维护备份；在线备份、多 Blob capture 等复杂协议在相应能力启用时再实现。
- AI 先通过 CLI/MCP 接续现有会话中的任务；主动创建、恢复或控制 Agent 是独立的后续执行增强。
- AI 输出是进度和完成候选，Done 由用户明确验收产生。
- 项目约束、关键决策和操作说明先作为带来源的简单记录复用；完整 BusinessFact、映射、漂移和 Optimizer 不属于 V1 发布承诺。
- 默认本地存储、无遥测，不为潜在分析用途默认采集完整会话。

## 阶段与发布范围

| 阶段 | 用户结果 | 发布定位 |
|---|---|---|
| A | 当前目录开始任务，保存进度与 Checkpoint，人工确认完成 | 首个可用版本 |
| B | 隔日、重启、切换 Worktree 后可靠接续并复核证据 | 日常使用版本 |
| C | 新的现有 AI 会话读取相同任务并继续工作 | V1 核心接入范围 |
| D | 按已验证需求控制一个 Runtime 并处理异常恢复 | 可选执行增强，不阻塞 V1 |
| X | 结构化业务事实、实现漂移、知识提炼和优化候选 | 后续探索，另立项 |

详细进入条件和退出条件见[实施路线](11-roadmap.md)。版本号在发布时确定，不把旧 0.x 阶段编号继续作为交付承诺。

## 文档目录

1. [产品定位](01-product-positioning.md)
2. [需求与验收](02-requirements.md)
3. [总体架构](03-architecture.md)
4. [安全与权限](04-security-model.md)
5. [领域与数据模型](05-domain-model.md)
6. [CLI 与 MCP](06-cli-and-mcp.md)
7. [Git 治理：后续能力](07-git-governance.md)
8. [Runtime 控制：阶段 D](08-runtime-adapters.md)
9. [数据、隐私与存储](09-session-data-and-privacy.md)
10. [Optimizer：探索 X](10-self-optimization.md)
11. [实施路线](11-roadmap.md)
12. [待决策事项](12-open-decisions.md)
13. [产品架构图](13-product-architecture.md)
14. [核心工作流](14-core-workflow.md)
15. [客户端架构](15-tui-gui-interaction-architecture.md)
16. [客户端协作流程](16-tui-gui-interaction-flow.md)
17. [任务核心架构](17-task-manager-architecture.md)
18. [任务生命周期](18-task-management-workflow.md)
19. [知识工作台探索](19-business-fact-workbench-architecture.md)
20. [事实与 Git 流程探索](20-business-fact-and-git-flow.md)
21. [接续、证据与验收补充合同](21-continuity-and-review-contracts.md)

## 本轮文档整理（2026-09-08）

已补齐第 21 篇及需求、领域模型、CLI、路线图的交叉约定：最小证据、Review 撤回、结构化阻塞、有界上下文、接续 Skill、接入验收分层与只读诊断。第 08 篇仍仅属于可选阶段 D，恢复目标问题保留在 D-022。

当前结果是设计文档，未交付 V1 程序或通过运行时验收。编码前仍须按第 12 篇关闭技术栈/核心部署、最小生命周期、restore generation 与证据物理存储等适用决策；V0 代码和测试不替代这些工作。
