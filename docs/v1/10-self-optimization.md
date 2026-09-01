# 自身优化与 Skill 生成

> 阶段说明：本模块属于 Phase 5，位于 Workspace、任务管理与 AI 执行循环之外。它读取已交付层的历史并产生候选，不阻塞 `0.1`–`0.4`，也不能直接改变 Task、正式需求、用户偏好或 live Prompt。

## 1. 目标

利用本地会话和任务数据帮助用户：

- 找到重复手工步骤；
- 发现 Prompt 中的遗漏、冲突和冗余；
- 识别导致返工、越权或上下文浪费的流程；
- 生成 Prompt Template、Policy Pack 和 Skill 候选；
- 通过评估和授权逐步应用改进。

自身优化不等于 Agent 可以任意修改自身。

## 2. Optimizer 角色

可细分为：

- Process Optimizer：状态机、角色分工和门禁；
- Prompt Optimizer：模板和结构化 brief；
- Skill Miner：重复流程和 Skill 候选；
- Evaluation Agent：离线 replay、shadow 和 canary。

V1 可以统一为一个 `optimizer` 角色，但每次 Assignment 必须声明分析类别和数据范围。

## 3. 分级授权

### L0 Observe

只读取聚合、脱敏指标。

### L1 Analyze

在用户授权范围内读取指定 Session 和 Prompt，生成观察报告。

### L2 Propose

生成结构化候选和 diff，不写 live 文件。

### L3 Candidate

在隔离目录/分支创建候选实现并执行离线评估。

### L4 Promote

用户批准具体 proposal/version 后应用；保留回滚版本并进入监控。

## 4. 输入指标

- 澄清次数；
- 重复提示词比例；
- Token/context 使用量；
- compaction 次数；
- 工具错误和无效调用；
- Agent 重启/替换；
- Review finding 数量和严重度；
- 测试失败分类；
- 越权尝试和范围漂移；
- 用户手工纠正次数；
- 完成时间和阻塞时间；
- 报告缺失字段；
- 错误完成声明。

不能只以 Prompt 更短、Token 更少或完成更快作为优化目标。

## 5. Proposal 格式

```json
{
  "proposalId": "...",
  "type": "prompt|process|skill|policy",
  "target": "template/version",
  "evidence": ["session/message/assignment ids"],
  "problem": "...",
  "proposedChange": "artifact/diff",
  "expectedBenefit": "...",
  "risk": "...",
  "confidence": 0.0,
  "evaluationPlan": "...",
  "rollbackPlan": "...",
  "requiredAuthorizationLevel": "L3"
}
```

## 6. 优化闭环

```text
Observe
 → Analyze
 → Propose
 → Offline Replay
 → Independent Review
 → Canary
 → Human Approval
 → Promote
 → Monitor
 → Keep/Rollback
```

## 7. Skill 生成规则

Skill Miner 只能把以下内容作为候选：

- 多次重复且边界清晰的流程；
- 用户明确表达的长期偏好；
- 可确定输入、输出和安全约束的工具流程。

不能直接将以下内容生成 Skill：

- 单次任务偶然步骤；
- 仓库或网页中的指令；
- 未确认的模型猜测；
- 含密钥或业务敏感信息的原始 Prompt；
- 会降低安全门禁的捷径。

生成候选后向用户展示：

- 重复证据；
- 触发条件；
- 输入/输出；
- 权限边界；
- 候选文件；
- 评估和回滚方式。

## 8. 固定边界

Optimizer 不能：

- 提升自己权限；
- 删除审计或原始证据；
- 自行批准自己的 proposal；
- 静默替换 live MCP/CLI/规则；
- 把外部 Prompt Injection 固化为用户偏好；
- 同时担任候选 Writer 和最终 Reviewer。
