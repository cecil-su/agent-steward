# Agent Steward Web（只读工作台）

独立 React + TypeScript + Vite 工程，使用 TanStack Query、Tailwind CSS、Zustand 和本地可维护的 shadcn 风格基础组件（Radix Slot/CVA）。保持绿色视觉与共享设计 token，不复刻原页面结构。

## 产品边界

**Web 永久只读，不再迁移原 UI 的业务写入口。** 创建/编辑/关闭任务、记录进展、关联调整、组件/源码维护等由 AI 使用已有 CLI 完成；后端写合同没有删除或放宽。

当前已提供：

- 任务状态视图、关键词搜索、分页与基础详情。
- 项目列表、按唯一名称或 `##ID` 精确查找、详情和分页历史。
- 项目→关联任务筛选、任务→所属项目导航及清除筛选。
- Checkpoint、近期备注/截断提示、完整备注、Session、任务历史、代码现场只读展示。
- 重新读取后复制任务上下文，并始终保留可选取的手工复制文本。
- 页面加载/刷新时自动 GET 检查本机授权或既有 Cookie；有效则直接进入，未授权/读取失败才显示手动连接表单，不自动发送登录 POST。
- 一次性 `#connect` 链接先从地址栏清除，再通过专用头兑换一次；StrictMode 不重放兑换，失败回到手动连接，不确定结果须 GET 核对。
- Cookie 登录/退出、认证失效清缓存、连接代次和不确定认证结果保护；主动退出后同一页面不自动重连。
- 带 API 合同头的 SSE GET 流、503 重连、刷新单飞合并、搜索/复制输入保护。
- 独立 UI release 检查；忙碌/有输入时保留页面，暂缓后须显式确认采用新版。

`src/lib/api.ts` 只导出 `get/connect/login/logout`，没有业务 POST 方法。登录/退出不会自动重试；不确定时仅允许显式 GET 核对当前授权，再恢复交互。UI 按钮隐藏不是服务端权限边界，仍需沿用现有认证/reader 合同。

**构建不会自动发布或替换正式 UI，正式操作须单独授权。** Schema5 已实现独立项目简介/架构/开发验证资料，由 CLI 使用项目 revision 和来源任务 version 维护，保存依据及 before/after 历史；项目、任务概览及复制上下文展示资料。引用校验不代表内容已自动验证，资料不是实时现场；缺少字段的旧后端与尚未维护的 null 明确区分。源码上下文查询及 Session Hook/导入元数据的进一步只读入口尚未接入。

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
- `src/features/workspace.tsx`、`detail-panels.tsx`：受控只读展示，不请求 API、不持有业务状态。
- `src/lib/api.ts`：同源认证、合同/CSRF头、错误分类、取消与连接代次保护。
- `src/lib/query-client.ts`：服务端快照缓存，不是第二状态源；关闭隐式 focus/reconnect 刷新。
- `src/stores/workspace.ts`：仅临时选择、搜索草稿、筛选，不保存任务/项目或凭据。
- `src/lib/live-events.ts`、`hooks/use-live-updates.ts`：GET-only 重连、单飞刷新与生命周期隔离。
- `src/lib/ui-release.ts`：版本检测与输入保护，不执行发布/回退或业务写入。
- `scripts/check-dist.mjs`：三文件、UTF-8、4 MiB上限、HTML资源/JS模块/常见Worker门禁；不是恶意JS沙箱，别名/计算属性加载仍需源码审查。

任务 notes/history/Session API 当前无分页，不伪造分页参数；项目历史按 revision 游标分页。代码现场未提供或读取失败时不显示为 clean。SSE 刷新不会替换搜索草稿；查询失败保留错误提示，不宣称旧快照为已验证现状。

## 构建与部署边界

`npm run build` 只写 `web/dist/{index.html,app.js,style.css}`，不覆盖 `crates/server/web/`，不激活正式发布。Windows `ui.ps1 Build` 读取 `web/dist`；**构建成功不代表正式网页已更新**。后端发布前显式执行 `npm run sync:embedded`，同步到 `crates/server/web-readonly` 后重新编译 taskd，保证内嵌回退也只读；旧 `crates/server/web` 不再作为活动资源入口。

当前 UI 请求头及外置包使用 API 合同 `2`（包格式仍为 `1`），需要配套合同 `2` 的 taskd，不能纯 UI 更新到合同 `1` 服务。CI 与 Windows 发布流程固定 `.nvmrc`，先运行前端单测/类型检查/构建并同步内嵌快照，再编译 Rust；浏览器 smoke 验证实际只读入口，不操作旧可写页面的按钮。CI 使用显式安装的 Playwright Chromium（`STEWARD_BROWSER_CHANNEL=chromium`）。

`npm run dev` 是本机前端开发服务，不提供业务 API、不配置正式地址 proxy，也不放宽 taskd Host/Origin/CSP。业务验证使用下方新隔离 taskd 托管构建结果。

## 真实浏览器验证

明确指定可信开发 `taskd/taskctl` 目录，本轮资料 smoke 要求 Schema5 项目资料 CLI/API 和独立 UI 包。脚本不会退回全局安装；新建临时库/runtime/UI包、随机端口、严格认证 reader，使用本机已安装 Chrome。缺浏览器直接失败，不下载。

```sh
# 先 npm run build；地址必须是本机网卡，以下仅限获准的隔离测试。
STEWARD_TEST_BIN_DIR=E:/path/to/development/debug \
STEWARD_TEST_BIND=172.19.10.185 \
STEWARD_BROWSER_CHANNEL=chrome \
fnm exec --using=24.11.1 npm.cmd run test:browser
```

监听指定网卡随机端口（HTTP，无传输加密），不访问既有实例，不改系统代理/防火墙。临时库通过 CLI 预置合成项目、任务、Checkpoint、备注、Session 及有来源的项目资料；浏览器只发 login/logout 两种 POST，其余均为 GET。用专属 shutdown marker 正常退出，不强杀；失败保留现场并报告路径。

覆盖桌面/390px、无业务按钮、CSP、reader登录退出、任务/项目/历史/上下文读取、项目查找筛选、503 SSE重连与搜索保护、临时 UI 更新提示保护及取消、项目资料/来源显示与复制、Task/Project/Profile/History不变。输出绑定 Node/Chrome、开发二进制 SHA、UI release 及 `.artifacts/<时间>/` 截图；测试过程中 SSE 用外部 CLI 创建额外合成任务，不把它误算为浏览器写入。

隔离验证使用 `.local/task45-fix-target/debug` 的 Schema5/API 合同 2 开发二进制，React 内嵌与外置包浏览器 smoke 均通过；前端 74 项单测、类型检查和构建通过。不声称完整业务人工验收、远程 CI 执行或正式部署。Windows 打包/启动脚本按 Schema5 校验，新旧 schema 不能通过普通更新混用；须先单独停写、备份、显式迁移并核验。参见[项目资料与显式离线复制合同](../docs/v0/19-Schema5项目资料与CLI维护.md)。

依据：[UI包合同](../docs/v0/14-UI独立发布.md)、[Vite](https://vite.dev/config/build-options)、[shadcn](https://ui.shadcn.com/docs/installation/vite)、[TanStack Query](https://tanstack.com/query/latest/docs/framework/react/reference/QueryClient)。
