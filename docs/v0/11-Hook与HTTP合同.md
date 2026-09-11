# Hook 与 HTTP 合同

本文描述V0当前入口；V1为独立设计。使用与隔离验证见[服务指南](12-服务使用与验收.md)。

## 显式绑定与可观察事件

### 身份与生命周期

- `session bind <session-id> --source <source> --external-session <id> --if-version <v>`：给已存在且未结束、Task 未关闭的本地 Session 一次性绑定来源。已有同值绑定是当前版本 no-op；不同绑定拒绝。`source + externalSessionId` 全库唯一。绑定写 Task version 和 History；不改变当前 Session。
- `claim/resume` 保持原合同。客户端的 started/resumed/idle/closed 是观测事件，绝不映射为 claim/resume/session close。失联和迟到事件不能回收执行权。
- 每次事件必须携带明确本地 Session ID、来源和外部 ID，与绑定完全匹配；不根据 cwd、最新 Task 或当前窗口猜测归属。客户端同一会话不能自动跨 Task 改绑。

### 接收合同

`hook ingest --input <file>` / `--json --input -` 接收一个严格 JSON：

```json
{"schemaVersion":1,"sessionId":"session-a","source":"generic","externalSessionId":"client-a","eventId":"event-1","kind":"idle","occurredAt":"2026-09-07T00:00:00Z"}
```

- `kind` 为 started/resumed/idle/closed/user_message/assistant_message/tool_call/tool_result/error。消息和工具事件首期只记录种类和时间，不保存正文、工具参数、结果、自由格式错误或附件；这些字段在适配器投影时丢弃，严格接收接口拒绝未知字段。保留身份字段只允许有界 ASCII 标识符。用户仍可用已审查的手工 Import 保存内容。
- 自动采集默认不启用；用户显式配置 Hook 和绑定后才启用。不得扫描客户端记录目录。客户端适配器必须从官方实际能力取事件，不能声称不支持的生命周期已接入。
- 单事件输入上限 16 KiB；日期必须为带时区 RFC3339，标准化为 UTC。标识符 1–128 字节，source 1–32 字节；禁止空白、路径、URL 和任意文本字段。不得将未知原文、输入全文或凭据写入错误、日志、Task、History 或临时重试文件。
- `(sessionId, eventId)` 唯一；相同标准化事件重放返回原记录，不新增；同键不同内容返回 `HOOK_EVENT_CONFLICT`。外部绑定先校验，不会因 ID 碰撞接受另一个来源。
- 事件独立于 Task mutation：追加不改变 Task、Session 的执行生命周期、Task version、updatedAt 或 History。已结束 Session 可接收迟到事件；观测日志不表示执行权。
- SQLite 事务保证校验、去重、插入和数量上限原子完成。每 Session 最多 10,000 条（含删除后的去重标记），满时返回 `HOOK_CAPACITY_REACHED`，不驱逐旧去重键。不提供无限队列或后台补偿。
- Hook 失败给出稳定错误并快速返回，不影响显式 CLI。自动包装器最多在短暂 busy 时重试两次；无落盘队列、无后台扫描；失败退出码和 stderr 让宿主显示丢失风险。接收失败不能被报告为已采集。
- `hook list <session-id> --after <sequence> --limit <1..200>` 按数据库 sequence 分页。展示发生时间和接收时间，乱序事件不重排本地执行状态。
- `hook clear <session-id> --if-version <v> --yes` 显式删除可见内容并留下最小去重标记，防止已删除事件被重试复活；递增 Task version，History 只保存 Session ID 和删除计数。使用 secure_delete 和 WAL checkpoint，保留既有物理擦除限制。

### 存储

Schema7的`session_events`外键指向Session，不是通用Event/Actor/Operation模型。普通连接拒绝不兼容格式，不隐式迁移或重建。JSON envelope为2，事件DTO自身schemaVersion为1。

## 本地服务与任务界面

- 单个 Rust Daemon 从启动参数固定数据库，提供同源静态 GUI 和 JSON API。默认绑定 `127.0.0.1`，默认端口 `43123`（`--port 0` 可显式选择临时端口）；用户可显式通过 `--bind <本机IPv4>` 绑定指定网卡地址，不允许通配、多播或广播地址，不接受请求切换数据库。Host/Origin 严格匹配实际监听地址和端口，不放宽 CORS。默认HTTP 80端口的Origin采用浏览器规范形式（不含`:80`），Host允许同一绑定IP带/不带`:80`；其它地址/端口不放行，重复Host仍拒绝。非回环 HTTP 不加密传输，启动时告警，使用者须限制网络访问；程序不自动修改防火墙。
- 所有业务经过Application Service；SQLite/Git同步工作在阻塞线程池执行。业务总并发8，读取最多占6，为写请求保留2个容量；读容量耗尽返回503。Cookie角色查询、授权发放与撤销使用独立4槽阻塞执行器，纯Token校验及默认本机信任不依赖认证SQLite槽。所有permit均由实际worker持有至完成，即使HTTP future取消也不提前释放；Git取消会终止所管理进程，SQLite quick_check等查询没有通用执行截止时间，不能把HTTP取消当作后台SQLite已停止。保留CAS、工作树锁、dirty检查和部分完成恢复合同。
- 首次启动创建管理员与只读两份随机凭据，写入用户专属 runtime/identity 目录并跨重启保留，普通 stdout/stderr 仅显示 URL 和凭据文件路径，不显示秘密。默认以真实 TCP peer 判定本机：回环地址或与监听网卡 IP 相同的直接来源可作为管理员，不需要 Cookie 或连接码。缺少 peer、含 Forwarded/X-Forwarded-* 的请求不获得本机特权；显式请求头凭据仍按其角色处理。此模式信任本机所有用户/程序，禁止通过本机代理、隧道或端口转发暴露；代理部署必须加 `--require-local-auth`。严格模式启动时才通过本机浏览器启动器传递随机一次性连接码（120 秒有效），页面立即清除 URL fragment，再通过 `POST /api/connect` 兑换浏览器授权 Cookie（不返回长期凭据）；码只能成功使用一次，兑换遵循同样的 Host/Origin 边界，不按来源 IP 放行。`--no-open` 禁止自动打开；其他设备使用只读凭据手动连接，自动连接失败时本机可手动使用管理员凭据；远程权限由凭据决定；本机免登录不因 Cookie 退出或撤销而关闭。浏览器使用随机独立授权 Cookie，`HttpOnly; SameSite=Strict; Path=/api; Max-Age=2592000`（30 天），服务端在私有 identity/browser-sessions.db 只保存 Cookie 哈希、角色、凭据绑定和过期时间。普通网址、新标签页及同 origin 重启可恢复；长期凭据不进入sessionStorage/localStorage或响应正文。退出登录删除对应授权并清除 Cookie；管理员可撤销当前 origin 全部浏览器授权；长期凭据轮换也使绑定的旧授权失效。当前只提供 HTTP，Cookie 不设 Secure，不能把 LAN HTTP 视作加密连接。一次性连接码仅放入本机启动链接的 fragment、不进入查询参数或日志。
- 每个 API 请求都检查精确 Host，存在 Origin 时必须匹配；拒绝 null/跨 Origin 和跨站 Fetch Metadata。除可信本机直接请求外，API 要求有效的请求头凭据或浏览器 Cookie（一次性连接端点使用连接码）。本机免登录或含 Cookie 的非 GET/HEAD 请求必须同时具有精确 Origin 与 `X-Steward-CSRF: 1`，POST 还要求 JSON Content-Type；请求头凭据若显式提供但错误，不回退到 Cookie。无 CORS 放行；静态资源禁止跨源嵌入，CSP 不允许内联脚本和第三方资源；用户内容仅作为文本渲染。
- API/CLI 共用数据库权限告警。GUI 显示 warnings 和结构化错误，认证失败回到连接界面。
- 请求体总上限1 MiB（Hook为16 KiB）；路径参数使用明确绝对路径，拒绝NUL及Windows设备/命名管道命名空间，不能使用Daemon cwd推断浏览器目录。没有HTTP文件正文下载端点，但管理员可在显式已审查确认后导入服务进程有权读取的普通文件（最多16 MiB）到SQLite，API显示路径、hash与大小；reader/匿名不能调用该写入。管理员Worktree及Import的普通UNC文件系统路径当前并未禁用，可能触发服务端网络访问，部署必须按管理员拥有这些主机文件/网络能力评估，不能宣称只允许浏览器本地文件。
- `taskctl --json` 和 HTTP业务JSON的成功、失败都使用 schemaVersion=2 envelope；不适用于独立宿主适配器 `task-hook`：generic成功为 `{ok:true,data:...}`，Codex成功或忽略事件为 `{}`，失败写stderr并非零退出，不为统一外形改变宿主协议。400 输入无效、401 认证失败、403 来源拒绝、404 不存在、409 CAS/Session/Hook 冲突、503 busy、500 其它失败；不得依赖自然语言解析错误。
- 所有写操作只在用户提交时发送一次。禁止自动重试 create/Worktree 等写请求；超时或断连提示“结果未确认，请刷新核对”。冲突保留用户输入并显示新版本，用户重新审查后提交；不能自动把最新 version 填回旧 Patch 重放。
- GUI 是只读工作台，提供任务/项目检索、详情、备注、Checkpoint、Session 历史、Hook观测、Import元数据、History、实时Worktree状态、项目资料与有效规则、上下文复制。不提供任务/项目维护、领取/恢复、关闭或Worktree业务写入口；业务维护使用CLI，HTTP写合同不因隐藏按钮而取消。认证连接/退出不属于业务写入。只读边界见[UI独立发布](14-UI独立发布.md)。
- 提供管理员/只读两种凭据；后端在读取写请求体和执行业务操作前拒绝只读凭据的业务非 GET/HEAD 请求；建立浏览器授权和退出登录例外不赋予写权限。通过认证 SSE 推送数据失效通知，页面重新查询快照；每秒观察 SQLite data_version，支持 CLI/Hook 外部提交。最多 16 条独立订阅，断线重连但不重放写请求，表单打开时只提示待刷新。保留手动刷新，不引入 WebSocket 或通用 Operation 模型。API 与 CLI 复用结果，GUI 不解析终端文本。

## 验证与限制

合同回归覆盖绑定、去重/冲突/墓碑、输入与容量、认证、Origin/CSRF、reader写入拒绝、CAS、Worktree部分完成及只读页面。入口见[测试与验收](08-测试与验收.md)和[原生适配指南](../../integrations/README.md)。

真实客户端信任配置、模型运行、目标平台和人工页面须独立验证；元数据适配器不提供完整聊天/附件归档或自动验收。
