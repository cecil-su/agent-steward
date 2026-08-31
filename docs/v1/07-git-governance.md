# Git 生命周期治理

## 1. 目标

允许用户逐步授权 AI 执行 Git 生命周期，同时保证：

- 操作范围绑定具体 Task/Repo/Worktree；
- 用户批准的事实和实际执行事实一致；
- 不覆盖用户未确认的工作区修改；
- push 与本地 commit/merge 分离；
- 崩溃和冲突可以协调恢复。

## 2. 权限策略

每项操作支持：

```text
deny
ask
allow-once
allow-task
allow-repo
allow-global
```

建议默认值：

| 操作 | 默认 |
|---|---|
| inspect/status/diff | allow-task |
| fetch | ask 或 allow-repo |
| 创建受管 Worktree | ask |
| 精确暂存 | ask |
| commit | ask |
| merge | ask |
| push | ask |
| force push/reset hard/clean | deny |

用户可以在实际使用中调整，但所有变化都要版本化和审计。

## 3. Plan → Approve → Execute → Verify

### Plan

冻结以下事实：

- repository canonical identity；
- Worktree realpath；
- branch/upstream；
- HEAD、target、merge-base；
- ahead/behind；
- status、index、untracked manifest；
- 精确文件范围和 blob；
- hooks/config/credential context；
- task stage、owner epoch、writer lease；
- 预期提交父、merge 父和 result tree。

生成 `planHash` 和 expiry。

### Approve

用户批准具体 planHash。批准不能泛化到变化后的 SHA 或其他 Worktree。

### Execute

执行前重新采集事实。任何差异都使计划失效，而不是自动重算并继续。

### Verify

- commit：父提交、patch、tree、hook、status；
- merge：父顺序、merge tree、冲突状态、clean；
- push：远程 ref、SHA、ahead/behind 和目标分支；
- Worktree 删除：远程结果已核验且用户策略允许。

## 4. 精确暂存

- 禁止默认 `git add -A`。
- Plan 记录允许路径和预期 blob/patch。
- 暂存后重新核对 index tree。
- Hook 自动修改文件时，计划进入 needs_reconciliation，不能静默提交漂移内容。
- `--no-verify` 必须是单独的单次授权并绑定具体 plan。

## 5. Worktree 安全

- 一个受管 Worktree 同时最多一个 Writer lease。
- 同一 branch 不作为多个并行写 Worktree 的常规方案。
- 主仓未跟踪文件不被临时集成操作触碰。
- 集成优先使用受管 isolation Worktree。
- canonical realpath 必须防 junction/symlink 越界。

## 6. 冲突处理

- CLI 不自动解决业务冲突。
- 记录 merge/rebase operation 现场及冲突文件。
- 将任务标记为 needs_reconciliation/blocked。
- 派发有范围的 Writer 处理。
- 修复后重新 Review、测试并生成新计划。
- 不使用 reset hard 或 clean 作为自动恢复手段。

## 7. Push 与凭据

Hardened 模式中：

- Git 凭据只在 taskd credential broker 可用；
- Agent 环境不继承 SSH Agent、Token 或 Credential Manager 权限；
- push 必须使用已批准 plan；
- force push 默认永久禁止；
- 推送失败保留分支和 Worktree。

如果 Agent 与用户共享凭据并可直接执行原生 Git，工具只能提供流程门禁，不能保证防绕过。
