# CLI 与 MCP 设计

> 本文命令和工具名是 V1 候选契约，编码前仍可调整。

## 1. 设计原则

- CLI 与 MCP 共用 Application Service、状态机和 Policy Engine。
- CLI 不通过修改文件实现任务操作。
- MCP 不通过 shell-out CLI 并解析文本实现业务逻辑。
- 人类和 AI 可以使用同一 CLI，但权限来自 connection principal/capability。
- MCP 工具隐藏是 UX，不是安全边界。
- 所有输出提供稳定 JSON 模式和关联 ID。

## 2. CLI 命令族

### 身份和会话

```bash
stewardctl session attach
stewardctl session whoami
stewardctl session list
stewardctl session show <session-id>
stewardctl role accept <grant-id>
```

### 任务

```bash
stewardctl task list [--status ...] [--json]
stewardctl task show <task-id-or-external-id>
stewardctl task confirmations <task-id>
stewardctl task create
stewardctl task transition <task-id> --to <status> --expected-version <n>
stewardctl task assign <task-id> --session <id> --role task-owner
stewardctl task audit [<task-id>]
```

### Assignment/Agent

```bash
stewardctl assignment create <task-id> --role writer --runtime herdr
stewardctl assignment complete <assignment-id> --report <artifact-id>
stewardctl agent spawn <assignment-id>
stewardctl agent resume <assignment-id> --session <session-id>
stewardctl agent status <assignment-id>
```

### Git

```bash
stewardctl git inspect <task-id>
stewardctl git plan commit|merge|push <task-id>
stewardctl git approve <plan-id>
stewardctl git execute <plan-id>
stewardctl git verify <operation-id>
```

`approve` 只有在可信 Human principal 下才可用；Agent shell 中相同命令必须被服务端拒绝。

### 数据与优化

```bash
stewardctl data export|inspect|delete-session|retention
stewardctl optimize observe
stewardctl optimize propose
stewardctl optimize experiment
stewardctl optimize promote <proposal-id>
```

## 3. MCP 工具候选

### 只读

```text
session_whoami
task_list
task_get
task_get_confirmations
task_get_next
task_audit
event_inspect
agent_get_status
git_inspect
optimization_get_proposals
```

### 受控写入

```text
task_create
task_transition
task_assign_owner
assignment_create
assignment_complete
event_ack
git_plan_create
git_plan_execute
optimization_propose
```

不提供：

- 任意 SQL；
- 任意文件写入；
- 任意 Git 命令；
- 由调用者指定“我是 registry-main”；
- 不带 expectedVersion 的覆盖式更新。

## 4. MCP Connection 绑定

MCP Server 启动时由可信宿主或 taskd 绑定：

- principalId
- sessionId
- invocationId
- grant/capability
- runtime identity

这些事实不能由普通 tool arguments 覆盖。

## 5. 错误契约

错误应结构化，例如：

```json
{
  "ok": false,
  "error": {
    "code": "STALE_OWNER_EPOCH",
    "message": "Task owner grant is no longer current",
    "currentVersion": 18,
    "recovery": "Refresh task facts and request a new grant"
  }
}
```

关键错误类别：

- UNAUTHENTICATED
- FORBIDDEN_SCOPE
- STALE_VERSION
- STALE_OWNER_EPOCH
- LEASE_CONFLICT
- INVALID_TRANSITION
- PLAN_EXPIRED
- GIT_FACT_CHANGED
- APPROVAL_REQUIRED
- RUNTIME_CAPABILITY_UNAVAILABLE
- NEEDS_RECONCILIATION

## 6. 幂等

创建 Assignment、完成事件、ack 和 Git execute 必须支持 idempotency key。同一 key 重试返回原结果，而不是重复执行。
