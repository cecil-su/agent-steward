# 会话数据、隐私与本地存储

> 阶段说明：`0.1` 只要求任务历史、评论、附件和本地备份；完整 AI Session Store、跨宿主 importer、Prompt 归档和加密 Blob 在 AI/优化阶段增量加入。

## 1. 数据目标

AI 与优化阶段默认完整保留用户选择纳管的本地会话，以支持：

- 跨会话恢复；
- 用户意图和偏好分析；
- 重复工作流识别；
- Prompt、流程和 Skill 优化；
- 失败、返工和权限事件审计。

默认不上传遥测或会话数据。

## 2. 数据分层

### SQLite 元数据

- Task/Session/Invocation/Assignment；
- RoleGrant/Capability/Lease；
- Prompt 版本和 hash；
- Tool call 索引和统计；
- Git plan/operation；
- Review/Test/Optimization 索引。

### 加密 Blob Store

- 完整原始会话；
- 完整 PromptInstance；
- 大型 tool output；
- 报告、截图和附件；
- Context checkpoint；
- 导入的宿主 Session 文件。

### 衍生索引

- 全文索引；
- embedding（可选、本地生成）；
- 用户偏好候选；
- 重复流程聚类；
- Prompt 质量指标。

衍生数据必须可从原始数据重建，并记录生成器版本。

## 3. Provenance

所有消息和内容必须标记来源：

```text
user
assistant
system
developer
tool
repository
external-web
runtime
subagent
```

只有明确的 `user` 内容可以直接作为用户意图证据。repository、web 和 tool 内容可能包含 Prompt Injection，不得自动转为长期偏好、规则或 Skill。

## 4. 用户意图分层

### Confirmed Preference

用户明确确认的长期偏好。

### Candidate Preference

重复出现但未确认的行为模式，需要向用户确认。

### Task-local Context

只属于当前任务，不可自动提升为长期偏好。

每条偏好保留 evidence message IDs、创建方式和用户确认记录。

## 5. 隐私与加密

- 数据默认仅绑定本机用户。
- Blob Store 默认加密；密钥由 OS Keychain 或用户口令保护。
- 数据库至少保护敏感字段；是否采用 SQLCipher 属于待决策项。
- Secret/token/cookie 检测用于降低意外暴露，但不应破坏原始审计能力。
- Optimizer 默认读取聚合与脱敏数据；读取完整 Session 需要额外授权。

## 6. 用户控制

必须提供：

```text
data status
data export
data inspect
data delete-session
data delete-task
data retention
data reindex
data telemetry-status
```

删除操作应说明：

- 删除原始 Blob；
- 删除或重建衍生索引；
- 审计日志中保留最小删除事实，不保留已删除正文。

## 7. 保留策略

虽然默认目标是保留全部会话，仍需支持：

- 永久保留；
- 按任务/项目保留；
- 按时间清理；
- 用户标记不可删除；
- 敏感 Session 单独删除。

## 8. 无遥测承诺

公开版本默认：

- 不发送使用统计；
- 不上传 crash log；
- 不上传 Prompt/Session；
- 不调用远程 embedding；
- 任何联网分析都需要显式启用并展示目标、范围和数据类型。
