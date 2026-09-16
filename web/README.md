# Agent Steward Web（只读工作台）

独立 React + TypeScript + Vite 工程，使用 TanStack Query、Tailwind CSS、Zustand 和本地可维护的 shadcn 风格基础组件（Radix Slot/CVA）。保持绿色视觉与共享设计 token，不复刻原页面结构。

## 产品边界

**Web 永久只读，不再迁移原 UI 的业务写入口。** 创建/编辑任务、变更状态、记录进展、关联调整、组件/目录资料维护等由用户或 AI 使用 CLI 完成。状态统一使用 `task status TASK STATUS --if-version VERSION`；claim 仅管理 Session，不改变状态。旧 close/block/unblock/pending-release/continue 命令不再使用。

当前已提供：

- 七状态：backlog 暂不开始、todo 等待开始（默认）、in_progress 执行中、in_review 待审核或验收、blocked 受阻、done 已完成、cancelled 不再推进。任意状态转换由用户或 AI 通过 CLI 完成。页面支持七状态及 active/recent 筛选；backlog/todo/done/cancelled 使用 status 参数，其余使用 view。
- 项目列表、按唯一名称或 `##ID` 精确查找、详情和分页历史。
- 项目→关联任务筛选、任务→所属项目导航及清除筛选。
- Checkpoint、近期备注/截断提示、完整备注、Session 和任务历史只读展示。Session 与任务状态独立，无 Session 写入口。
- 历史 closureOutcome/closureReason/closedAt 独立展示，不代表当前状态，不与 done/cancelled 联动。Checkpoint gitHead 仅作为历史数据保留。
- 无 Worktree、代码现场或源码文件读取入口/API 调用。SourceRootView 仅包含 id/projectId/componentId/directoryPath/createdAt；目录为资料，不验证存在性、可访问性或读取文件。
- 目标、范围、验收、下一步、Note 与 Checkpoint 统一按安全 Markdown 展示，支持标题/列表/引用/代码块/表格/链接，保留普通文本换行；存储与上下文复制仍保留原文。原始 HTML 显示为文本，只允许 http/https/mailto 链接，不加载 Markdown 图片。代码块和表格局部横向滚动。
- 重新读取后复制任务上下文，并始终保留可选取的手工复制文本。
- 页面加载/刷新时自动 GET 检查本机授权或既有 Cookie；有效则直接进入，未授权/读取失败才显示手动连接表单，不自动发送登录 POST。
- 一次性 `#connect` 链接先从地址栏清除，再通过专用头兑换一次；StrictMode 不重放兑换，失败回到手动连接，不确定结果须 GET 核对。
- Cookie 登录/退出、认证失效清缓存、连接代次和不确定认证结果保护；主动退出后同一页面不自动重连。
- 带 API 合同头的 SSE GET 流、503 重连、刷新单飞合并、搜索/复制输入保护。
- 独立 UI release 检查；忙碌/有输入时保留页面，暂缓后须显式确认采用新版。

`src/lib/api.ts` 只导出 `get/connect/login/logout`，没有业务 POST 方法。登录/退出不会自动重试；不确定时仅允许显式 GET 核对当前授权，再恢复交互。UI 按钮隐藏不是服务端权限边界，仍需沿用现有认证/reader 合同。

**构建不会自动发布或替换正式 UI，正式操作须单独授权。** 项目简介、架构和开发验证资料由 CLI 使用项目 revision 和来源任务 version 维护，保存依据及 before/after 历史；项目、任务概览及复制上下文展示资料。引用校验不代表内容已自动验证，资料不是实时现场；缺少字段的旧后端与尚未维护的 null 明确区分。源码上下文/文件读取不属于本 UI 范围。

## 工具链

仓库根 `.nvmrc` 固定 Node **24.11.1**。进入目录是否自动切换取决于用户现有 fnm/nvm shell 钩子，不修改全局默认 Node。

```sh
# 仓库根
fnm use
cd web
node --version
npm ci --ignore-scripts
npm run typecheck
npm test
npm run build
```

Windows 工具 shell 可显式使用：

```sh
fnm exec --using=24.11.1 npm.cmd run build
fnm exec --using=24.11.1 npm.cmd test
```

依赖精确锁定；无安装生命周期脚本或自动浏览器下载。不跨平台复制 `node_modules`，TypeScript/Vite/Tailwind 含平台依赖。

## 代码与状态

- `src/components/ui/`、`src/styles.css`：统一尺寸/颜色/间距及基础组件。
- `src/lib/markdown.ts`、`src/markdown.css`：React 与原生页面共用的 Markdown 解析、安全清洗和样式。`npm run build:native-markdown` 显式更新原生 `app.js/style.css` 的生成前缀，不更改其手写正文、预览、发布包或内嵌资源。修改共享渲染器/样式后需运行该命令并提交生成结果；原生包仍仅使用三个页面资源文件。
- `src/features/workspace.tsx`、`detail-panels.tsx`：受控只读展示，不请求 API、不持有业务状态。
- `src/lib/api.ts`：同源认证、合同/CSRF头、错误分类、取消与连接代次保护。
- `src/lib/query-client.ts`：服务端快照缓存，不是第二状态源；关闭隐式 focus/reconnect 刷新。
- `src/stores/workspace.ts`：仅临时选择、搜索草稿、筛选，不保存任务/项目或凭据。
- `src/lib/live-events.ts`、`hooks/use-live-updates.ts`：GET-only 重连、单飞刷新与生命周期隔离。
- `src/lib/ui-release.ts`：版本检测与输入保护，不执行发布/回退或业务写入。
- `scripts/check-dist.mjs`：三文件、UTF-8、4 MiB上限、HTML资源/JS模块/常见Worker门禁；不是恶意JS沙箱，别名/计算属性加载仍需源码审查。

任务 notes/history/Session API 当前无分页，不伪造分页参数；项目历史按 revision 游标分页。不请求代码现场，也不在复制上下文中包含现场项。SSE 刷新不会替换搜索草稿；查询失败保留错误提示，不宣称旧快照为已验证现状。

## 构建与部署边界

`npm run build` 只写 `web/dist/{index.html,app.js,style.css}`，不覆盖 `crates/server/web/`，不激活正式发布。Windows `ui.ps1 Build` 读取 `web/dist`；**构建成功不代表正式网页已更新**。后端发布前显式执行 `npm run sync:embedded`，同步到 `crates/server/web-readonly` 后重新编译 taskd，保证内嵌回退也只读；旧 `crates/server/web` 不再作为活动资源入口。

本目录源码请求 API 合同 `5`（包格式仍为 `1`），需配套 Schema8/合同5 的 taskd；JSON envelope schemaVersion 为 `3`，由后端提供。不能将候选 UI 激活到旧合同服务。本目录 React 构建与 `crates/server/web-legacy-readonly` 原生源码分别验证，不自动替换正式外置页面或内嵌回退。任务/项目概览及任务上下文复制完整展示有效 sessionRules、revision与来源；空规则和规则不可用分开，不提供规则业务写入口。CI 与 Windows 发布流程固定 `.nvmrc`，先运行前端单测/类型检查/构建并同步内嵌快照，再编译 Rust；浏览器 smoke 验证实际只读入口，不操作旧可写页面的按钮。CI 使用显式安装的 Playwright Chromium（`STEWARD_BROWSER_CHANNEL=chromium`）。

`npm run dev` 是本机前端开发服务，不提供业务 API、不配置正式地址 proxy，也不放宽 taskd Host/Origin/CSP。业务验证使用下方新隔离 taskd 托管构建结果。

## 真实浏览器验证

明确指定可信开发 `taskd/taskctl` 目录，smoke 要求 Schema8、新 task status CLI 和合同5独立 UI 包。脚本不会退回全局安装；新建临时库/runtime/UI包、随机端口、严格认证 reader，使用本机已安装 Chrome。缺浏览器直接失败，不下载。

```sh
# 先 npm run build；地址必须是本机网卡，以下仅限获准的隔离测试。
STEWARD_TEST_BIN_DIR=E:/path/to/development/debug \
STEWARD_TEST_BIND=172.19.10.185 \
STEWARD_BROWSER_CHANNEL=chrome \
fnm exec --using=24.11.1 npm.cmd run test:browser
```

监听指定网卡随机端口（HTTP，无传输加密），不访问既有实例，不改系统代理/防火墙。临时库通过 CLI 预置合成项目、任务、Checkpoint、备注、Session 及有来源的项目资料；浏览器只发 login/logout 两种 POST，其余均为 GET。用专属 shutdown marker 正常退出，不强杀；失败保留现场并报告路径。

覆盖桌面/390px、无业务按钮、CSP、reader登录退出、任务/项目/历史/上下文读取、项目查找筛选、503 SSE重连与搜索保护、临时 UI 更新提示保护及取消、项目资料/来源显示与复制、Task/Project/Profile/History不变。输出绑定 Node/Chrome、开发二进制 SHA、UI release 及 `.artifacts/<时间>/` 截图；测试过程中 SSE 用外部 CLI 创建额外合成任务，不把它误算为浏览器写入。

Schema8/合同5开发二进制隔离 smoke 已通过：Node24.11.1、Chrome153.0.8010.48，绑定 `172.19.10.185`，使用 `E:/ai/agent-steward/target/debug`。React 截图位于 `.artifacts/2026-09-16T08-34-49.399Z/`；原生结果和截图位于 `.artifacts/native-2026-09-16T09-42-41.784Z/`。两者均使用临时库与专属 shutdown 标记正常退出，无正式数据或既有服务变更。前端 111 项测试、类型检查及构建通过；自动化通过不等于人工验收完成。

## 唯一人工预览入口

预览固定复用 `.local/local-preview`，地址 `http://172.19.10.185:60446`；当前使用合同4**原生页面和真实数据**，GET-only代理到 `http://172.19.10.185:51850`。原生源码与发布方法见[原生UI说明](../crates/server/web-legacy-readonly/README.md)。

- 当前只需一个Node预览进程；不额外启动合成taskd。`candidate/`合成库和日志保留为隔离证据，其后端已停止。
- `config.json`：bind `172.19.10.185`、port `60446`、upstream `http://172.19.10.185:51850`、apiContract `4`、uiVersion对应原生包版本、dataSource `real`。
- `scripts/local-preview-server.cjs`启动前检查上游合同；保留本机peer、精确Host、同源Origin/cross-site检查、GET-only和安全响应头，不转发Cookie、Authorization或Token。
- 更新预览前核实实际进程身份，通过 `.local/local-preview/stop` 正常停止；保存config/server/UI/state到 `backups/`，再更新预览并启动server.cjs。不可变发布包和正式服务不随预览改动；不采用旧PID、不强杀、不清理无关沙箱。
- `rules-preview.mjs`用于明确选择的React合成候选，不是当前原生真实数据预览的默认启动器。不能仅因资源变化就停止真实预览并改回合成数据，或以它替换正式原生页面。

合同5候选资源不兼容 Schema7/合同4 服务。仓库内嵌三文件已同步合同5并重新编译，通过隔离 embedded 浏览器 smoke，截图位于 `.artifacts/2026-09-16T08-43-15.699Z/`。构建、显式内嵌同步和发布分别处理，不自动更新正式服务或既有预览。人工验收仍待确认。

依据：[UI包合同](../docs/v0/14-UI独立发布.md)、[Vite](https://vite.dev/config/build-options)、[shadcn](https://ui.shadcn.com/docs/installation/vite)、[TanStack Query](https://tanstack.com/query/latest/docs/framework/react/reference/QueryClient)。
