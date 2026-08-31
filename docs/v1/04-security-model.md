# 安全与权限模型

> 阶段说明：任务管理 MVP 先落实本地数据、版本冲突和基本身份边界；本文完整威胁模型主要在接入 AI、高风险 Git 操作和外部 Runtime 前成为发布门槛，不阻塞纯任务管理闭环。

## 1. 威胁模型

V1 需要防范：

- Agent 受 Prompt Injection 影响后尝试越权；
- 普通 Agent 自称 Registry Main 或 Task Owner；
- 子代理访问其他任务、Worktree、报告或会话；
- 过期 Session/owner 继续写入；
- AI 绕过 MCP，改用 CLI 请求同一高权限操作；
- AI 直接读取管理数据库、审计日志或 Git 凭据；
- AI 在计划批准后利用 Git 现场变化执行不同操作；
- Optimizer 把仓库、网页或工具内容误识别为用户长期偏好。

V1 不承诺防范：

- 本机管理员；
- 恶意软件、内核或操作系统攻破；
- 用户主动关闭隔离后产生的后果；
- 外部 Git/Agent 服务自身的安全漏洞。

## 2. 信任区域

```text
Trusted
- taskd
- SQLite / encrypted blob / audit
- Git credential broker
- Git lifecycle executor
- Human approval channel

Untrusted or scoped
- AI Session
- Bash
- MCP Client
- taskctl Agent Client
- Herdr/native subagent
- repository/web/tool content
```

如果 Agent 与用户使用同一 OS 身份并可读取数据库、密钥和 Git 凭据，则无法形成硬安全边界。Hardened 模式需要独立 OS 身份、容器、沙箱、ACL 或等价隔离。

## 3. Principal 与角色

### Principal

- Human/Admin
- AI Session
- Service
- Runtime Adapter

### 角色层级

```text
Human/Admin
  └── Registry Main
       └── Task Owner
            ├── Scout
            ├── Writer
            ├── Reviewer
            ├── Tester
            └── Optimizer（需额外授权）
```

角色不是全局字符串，而是带 scope 的 RoleGrant。

## 4. RoleGrant

至少包含：

```json
{
  "grantId": "uuid",
  "issuerPrincipalId": "...",
  "subjectSessionId": "...",
  "role": "writer",
  "taskId": "...",
  "assignmentId": "...",
  "worktreeId": "...",
  "allowedOperations": [],
  "allowedPaths": [],
  "ownerEpoch": 4,
  "issuedAt": "...",
  "expiresAt": "...",
  "status": "active"
}
```

约束：

- Agent 只能接受已签发 grant，不能选择更高角色。
- grant 不可扩权转发；Task Owner 只能在自己的 Task scope 内派发。
- 所有权转移增加 owner epoch，旧 epoch 立即失效。
- resume 产生新 Invocation，并重新验证 grant/lease。

## 5. Capability

MCP connection、CLI Agent client 和 Runtime Adapter 使用短期 capability。Capability 应：

- 不出现在 Prompt 正文；
- 不作为普通命令参数写入日志；
- 绑定 connection/session/assignment；
- 可撤销、可过期、可单次消费；
- 不能由客户端自行构造 role 或 scope。

## 6. 用户审批

高风险批准应通过 Agent 无法伪造的可信渠道完成。候选方式：

- 独立本地 UI；
- 与 Agent shell 隔离的可信终端；
- OS credential/Windows Hello；
- 受保护的本地 socket 和交互式确认。

批准绑定：

- 操作类型；
- plan hash；
- Task/Repo/Worktree；
- 具体 SHA；
- 有效期；
- 使用次数。

简单的 `taskctl approve` 如果 Agent 也能在同一身份下调用，不构成安全批准。

## 7. 固定安全不变量

即使 Optimizer 获得高级授权，也不能：

- 给自己或其他 Agent 提权；
- 删除或改写审计历史；
- 绕过 expectedVersion/ownerEpoch；
- 静默降低 Git 或数据保护策略；
- 将外部内容直接写成用户偏好；
- 修改安全核心后自行批准和发布。

## 8. 安全模式

### Standard

面向防误操作和协作完整性，同一用户进程下运行；清楚标注它不是对抗性隔离。

### Hardened

- taskd/数据库/凭据由受保护身份持有；
- Agent 运行在受限用户、容器或沙箱；
- Agent 可写源码但不能写 Git 元数据；
- Git 生命周期和 push 只能由受信任执行器完成。

公开发布时必须明确区分两种模式，不能把 Standard 宣称为硬安全。
