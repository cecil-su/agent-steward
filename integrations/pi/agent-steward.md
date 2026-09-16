# Pi Steward 管理适配候选

## 范围

`agent-steward.ts` 注册 `steward_task` 工具及 `/steward-status` 查询命令，面向 taskctl JSON envelope **3**。它不是 `steward.ts` Hook，未自动安装、迁移数据库或修改全局扩展。

支持 action：`create`、`show`、`claim`、`update`、`retitle`、`note`、`checkpoint`、`status`。不支持 `close`、`block`、`unblock` 或 Worktree 操作。

七种业务状态：`backlog`、`todo`、`in_progress`、`in_review`、`blocked`、`done`、`cancelled`。

## 操作约束

- 普通会话启动不查询数据库、不自动创建、关联、恢复或接管任务。工具仅在用户明确要求管理任务或已有明确关联时使用。
- `show` 只读取资料；仅当返回的 currentSessionId 属于当前 Pi Session 时，记住该关联。业务状态不构成执行权限。
- `claim` 仅调用 `task claim --session … --if-version …`，不改变业务状态。必须 `confirmedByUser=true`；接管还需显式 `takeOver=true`，只应在用户明确授权接管时提供。
- `status` 必须提供用户选定的 `status`、用户确认快照的 `expectedVersion` 和 `confirmedByUser=true`。仅调用 `task status <id> <status> --if-version <version>`。
- `update`、`retitle` 同样要求 `expectedVersion`、`confirmedByUser=true`；`update` 还要求非空 `reason`。确认针对具体任务、版本、补丁，不可从一般执行授权推断。
- 每次写入先读取任务。确认版本过期时仅返回刷新结果，不写入。写入发生 VERSION_CONFLICT 时再读取一次，绝不自动重试。
- Checkpoint 要求当前 Pi Session 所属的有效任务 Session，不依据业务状态推断权限。
- 标题使用 `MMDD｜类型｜主题`；日期按 Asia/Shanghai 会话日期填写，类型为功能、设计、修复、优化、发布、探索、文档、研究。
- `confirmedByUser` 是调用方对用户指令的显式声明，不是密码学授权或独立审批系统。

## 配置与手动加载

仅需配置以下环境变量（显式传入隔离值后再测试）：

| 变量 | 值 |
| --- | --- |
| `AGENT_STEWARD_TASKCTL` | 支持 envelope 3 的 taskctl 可执行文件绝对路径；未设时为 PATH 中的 `taskctl` |
| `AGENT_STEWARD_DATABASE` | 要访问的数据库绝对路径；未设时由 taskctl 选择默认数据库，可能是正式库 |

人工验收须先指定专用临时数据库，且确认不会同时加载旧全局 `agent-steward.ts`（同名工具冲突）。不要将正式库用于候选测试。可在隔离的 Pi 配置环境中显式加载：

```text
pi -e <仓库绝对路径>/integrations/pi/agent-steward.ts
```

持久安装由用户另行决定：将候选加入 Pi 的 extensions 配置或复制到受信任扩展目录后 reload；应先停用旧同名管理扩展。不执行这些安装步骤，也不替换 `integrations/pi/steward.ts`。

运行依赖为 Pi 0.85.1 的 `@earendil-works/pi-coding-agent` 类型、`@earendil-works/pi-ai`、`typebox` 和 Node 内置模块。命令采用无 shell 参数数组，JSON 从 stdin 传递。进程超时 30 秒、stdout/stderr 合计上限 4 MiB；工具文本输出上限 50 KiB/2000 行。超时或中断后写入结果可能未知，应先查询而非盲目重试。

## 隔离验证

```text
node --test integrations/tests/agent-steward.test.mjs
```

测试使用 Node 24 的 TypeScript 类型擦除及注入的进程 mock，不加载 Pi、不执行 taskctl、不访问任何数据库或全局扩展。Schema 构造依赖被替身替换，测试重点是实际工具执行分支的参数、确认门槛、CAS、Session/状态分离和错误不重试。

已验证：11 项契约测试通过。真实 Pi 加载、完整 TypeScript 类型检查及新二进制端到端安装尚未验证。
