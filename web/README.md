# Agent Steward Web（只读工作台）

独立 React + TypeScript + Vite 工程，使用 TanStack Query、Tailwind CSS、Zustand 和本地可维护的 shadcn 风格基础组件（Radix Slot/CVA）。保持绿色视觉与共享设计 token，不复刻原页面结构。

## 产品边界

**Web 永久只读，不再迁移原 UI 的业务写入口。** 创建/编辑/关闭任务、记录进展、关联调整、组件/源码维护等由 AI 使用已有 CLI 完成；后端写合同没有删除或放宽。

当前已提供：

- 任务状态视图、关键词搜索、分页与基础详情；“待上线”使用独立标识和筛选，计入未关闭，不计入进行中。
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

当前 UI 请求头及外置包使用 API 合同 `4`（包格式仍为 `1`），需要配套 Schema7/合同4 的 taskd，不能纯 UI 更新到合同1/2/3服务。旧原生及legacy只读包不作为Schema7发布入口。任务/项目概览及任务上下文复制完整展示有效 sessionRules、revision与来源；空规则和规则不可用分开，不提供规则业务写入口。CI 与 Windows 发布流程固定 `.nvmrc`，先运行前端单测/类型检查/构建并同步内嵌快照，再编译 Rust；浏览器 smoke 验证实际只读入口，不操作旧可写页面的按钮。CI 使用显式安装的 Playwright Chromium（`STEWARD_BROWSER_CHANNEL=chromium`）。

`npm run dev` 是本机前端开发服务，不提供业务 API、不配置正式地址 proxy，也不放宽 taskd Host/Origin/CSP。业务验证使用下方新隔离 taskd 托管构建结果。

## 真实浏览器验证

明确指定可信开发 `taskd/taskctl` 目录，smoke 要求 Schema7、项目资料 CLI/API 和合同4独立 UI 包。脚本不会退回全局安装；新建临时库/runtime/UI包、随机端口、严格认证 reader，使用本机已安装 Chrome。缺浏览器直接失败，不下载。

```sh
# 先 npm run build；地址必须是本机网卡，以下仅限获准的隔离测试。
STEWARD_TEST_BIN_DIR=E:/path/to/development/debug \
STEWARD_TEST_BIND=172.19.10.185 \
STEWARD_BROWSER_CHANNEL=chrome \
fnm exec --using=24.11.1 npm.cmd run test:browser
```

监听指定网卡随机端口（HTTP，无传输加密），不访问既有实例，不改系统代理/防火墙。临时库通过 CLI 预置合成项目、任务、Checkpoint、备注、Session 及有来源的项目资料；浏览器只发 login/logout 两种 POST，其余均为 GET。用专属 shutdown marker 正常退出，不强杀；失败保留现场并报告路径。

覆盖桌面/390px、无业务按钮、CSP、reader登录退出、任务/项目/历史/上下文读取、项目查找筛选、503 SSE重连与搜索保护、临时 UI 更新提示保护及取消、项目资料/来源显示与复制、Task/Project/Profile/History不变。输出绑定 Node/Chrome、开发二进制 SHA、UI release 及 `.artifacts/<时间>/` 截图；测试过程中 SSE 用外部 CLI 创建额外合成任务，不把它误算为浏览器写入。

## 唯一人工预览入口

`fnm exec --using=24.11.1 node web/scripts/rules-preview.mjs ABSOLUTE_CANDIDATE_BINARY_DIRECTORY` 仅管理已有 `.local/local-preview`，沿用其中 config.json 的 bind/port。完整工作台的唯一人工入口为该地址；当前为 `http://172.19.10.185:60446`，数据明确为**合成数据**，不是正式任务/真实偏好。

- 两个必要角色：原入口 Node 进程提供 UI 与 GET-only 代理；`candidate/taskd.exe` 仅在回环随机端口提供 Schema7/合同4合成后端，不作为第二个人工入口。数据库固定 `candidate/synthetic.db`，runtime、进程信息及专属停止标记均在同一目录中。重复运行核对实际进程身份、资源hash、合同与context后复用，不再新建 rules-live-preview 目录或重复监听。
- `config.json` 字段为 bind、port、upstream、apiContract:4、uiVersion:rules-candidate、dataSource:synthetic。代理实现为 `scripts/local-preview-server.cjs`，启动前检查真实upstream合同；保留本机peer、精确Host、同源Origin/cross-site检查、GET-only与安全响应头，不转发Cookie、Authorization或Token。旧合同UI请求拒绝，不仅替换manifest数字。
- 现有合同1正式后端与合同4 UI不能混用。候选只替换预览目录内的可变配置/UI，不改正式数据库、服务、全局扩展或不可变发布包。配置/server/UI及旧state保存于 `backups/<timestamp>`；旧沙箱不删除。状态文件只作定位线索，不能代替CIM/实际命令行及监听核验。
- 结束或刷新候选前，先核实两角色命令行及路径，再分别向 `.local/local-preview/stop` 与 `candidate/stop` 写入 `stop`，等待对应进程/监听正常退出，不强杀。刷新需重新运行上述命令；存在运行进程或资源变更时脚本不会自动重启。
- 恢复旧预览：确认候选两角色已退出，保留当前现场，再把选定备份的config.json、server.cjs及ui复制回**预览目录**并启动其中server.cjs。恢复旧合同UI与旧配置须成套，不恢复过期PID，不迁移/重启其正式upstream。候选库、日志和停止标记保留作证据。

Node24.11.1下78项前端测试、类型检查和构建通过；预览代理有独立合同/安全回归。完整工作台页面、规则来源及上下文复制仍待人工确认，静态面板不是验收。未默认运行完整browser-smoke；内嵌三文件已同步。正式迁移、安装与重启未执行，仍需另行授权。参见[规则与显式离线复制合同](../docs/v0/22-个人偏好与项目规则.md)。

依据：[UI包合同](../docs/v0/14-UI独立发布.md)、[Vite](https://vite.dev/config/build-options)、[shadcn](https://ui.shadcn.com/docs/installation/vite)、[TanStack Query](https://tanstack.com/query/latest/docs/framework/react/reference/QueryClient)。
