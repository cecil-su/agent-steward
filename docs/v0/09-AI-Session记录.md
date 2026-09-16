# AI Session 记录

## 数据与业务状态的边界

Session 保存显式会话身份与继续关系，不代表完整聊天、任务状态或执行授权。Task 的 currentSessionId 指向同任务尚未结束的 Session；latestCheckpointId 指向同任务检查点。所有七种任务状态均适用以下合同，done/cancelled 不例外。

SessionView：`id/taskId/source/externalSessionId/continuedFrom/recordPath/startedAt/endedAt`。可空字段输出 null；本地 ID 与宿主外部 ID 不混用。没有外部 ID 时使用稳定本地 ID，不从文件名或聊天猜测来源。

## 显式生命周期

| 操作 | 行为 |
| --- | --- |
| task claim | 创建尚不存在的本地 Session 并设为当前；不改 status |
| claim 同一当前 Session | 先检查 CAS；Session 尚未结束才 no-op |
| claim --take-over | 显式创建新 Session，continuedFrom 指向原当前 Session；旧记录保留，不伪造 endedAt |
| task resume | 显式创建新 Session，continuedFrom 指向指定来源；不改 status |
| session attach | 新建来源/记录路径元数据，不设为当前 Session；身份字段完全一致时当前版本 no-op，不重新激活历史 Session |
| session bind | 对已有未结束 Session 一次性绑定 source/externalSessionId；同值当前版本 no-op，不改当前关联 |
| session close | 设置 endedAt；仅当它是当前 Session 时同事务清空 currentSessionId；不改业务状态 |

claim 已有其他当前 Session 时必须显式 take-over。resume 来源优先 `--from-session`，否则使用当前 Session；没有来源则拒绝。来源必须属于同一 Task，新 ID 必须尚不存在且不同于来源。存在当前 Session 时 resume 必须显式 take-over。历史来源可以已结束，但新 Session 不能复用旧 ID。

无当前 Session 时可显式 claim 开始独立新会话，或 resume 延续明确来源；读取任务、宿主启动、窗口切换及状态修改均不得隐式领取、接管或恢复旧会话。不以超时、心跳或 Hook closed 自动回收会话。

所有相关写入使用最新 Task version，业务变更与 History 同事务。响应丢失先读取核对，不自动重放。

## CLI

以下 TASK/VERSION/SESSION 来自明确指定的隔离库，每次调用使用 `--database TEST_DB`：

```text
taskctl task claim TASK --session SESSION --if-version VERSION [--take-over]
taskctl task resume TASK --session NEW_SESSION --from-session OLD_SESSION --if-version VERSION [--take-over]
taskctl task checkpoint TASK --session SESSION --if-version VERSION --input checkpoint.json
taskctl session list [--task TASK]
taskctl session show SESSION
taskctl session attach TASK --session SESSION --if-version VERSION [--source CLIENT] [--external-session EXTERNAL_ID] [--record-path PATH]
taskctl session bind SESSION --source CLIENT --external-session EXTERNAL_ID --if-version VERSION
taskctl session close SESSION --if-version VERSION
taskctl session import add TASK --session SESSION --if-version VERSION --file FILE --confirm-sensitive-content-reviewed
taskctl session import list SESSION
taskctl session import remove IMPORT_ID --if-version VERSION --yes
```

## Checkpoint 与恢复上下文

```json
{"summary":"当前进展","completed":[],"decisions":[],"pending":[],"nextStep":"下一步","risks":[]}
```

summary/nextStep 及数组元素须非空，未知字段拒绝。Checkpoint 只接受同任务尚未结束的当前 Session，不以业务状态限制。保存时递增 Task version、更新 latestCheckpointId/nextStep 并写 History；检查点不可原地编辑，修正通过新检查点完成。

`gitHead` 仅保留历史值；新检查点为 null，不采集 Git。resume 返回 task/checkpoint/sessions/sessionRules/nextStep，不返回现场；sessions 按 startedAt ASC、id ASC。task context 是独立只读入口，不执行 resume，不读取源码或 Git。

## Session 内容与隐私

attach 的 recordPath 只是外部记录引用，存在时保存规范化路径，不存在时保存绝对弱引用并提示；不是源码读取能力。Import 只接受用户明确提供、已审查、不超过16 MiB的普通文件；固定缓冲区有界读取并计算 SHA-256。不得导入凭据或隐藏推理，数据库无内容加密保证。

`sessionId + sha256` 去重，重复且 CAS 正确时返回现有元数据、不改版本/History；元数据查询不返回 BLOB。remove 同事务逻辑删除、递增版本及写不含原文的 History；启用 secure_delete，提交后尝试 WAL truncate，不保证备份/SSD/快照上的物理擦除，也不删除外部原始文件。

## Hook

显式配置并绑定后才采集元数据。started/resumed/idle/closed 等事件不映射到 claim/resume/session close/status；迟到事件不激活旧会话。事件按独立键去重，不改变 Task version/History；正文与工具参数结果在投影时丢弃。详见 [Hook与HTTP](11-Hook与HTTP合同.md)、[宿主适配](../../integrations/README.md)。

## 验证

隔离测试覆盖七状态下 claim/attach/bind/close/resume/checkpoint/import、身份和接管限制、CAS与History失败回滚、Hook不改变生命周期。Session 可用、记录成功和测试通过均不等于业务验收；状态由用户通过 [task status](21-待上线任务状态.md) 单独决定。
