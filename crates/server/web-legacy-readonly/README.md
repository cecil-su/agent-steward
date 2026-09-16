# 原生只读 UI

本目录为正式原生 HTML/CSS/JS 源码，不是 React 构建输出。保持已确认的原生布局，不覆盖 `../web`、`../web-readonly` 或不可变发布包。

## 兼容与发布

- 本目录候选源码使用 API合同 **5**、JSON envelope schemaVersion **3**、包格式1，需配套 Schema8/合同5 taskd。不能激活到旧合同服务；构建不会自动更新 embedded、正式预览或部署。
- 正式地址：`http://172.19.10.185:51850`；外置UI版本 `native-readonly-20260910075151`，release `b74d4a62823b24aebeb83f63542ea1d39465b69ec992c5e6e4046879a738c2df`。
- 预览复用 `.local/local-preview`，地址 `http://172.19.10.185:60446`，代理正式合同4接口，**真实数据、GET-only**，不转发凭据。无需额外合成后端。
- 原生产生的包只能包含本目录 `index.html/app.js/style.css` 和按实际字节SHA生成的 `manifest.json`。正式包位置 `.local/favicon-cache-20260910075151`；不能只修改旧包合同号来冒充兼容包。
- `distribution/windows/ui.ps1 -Action Build` 当前读取React的 `web/dist`，**不要用它替代原生包**；原生包按[UI包合同](../../../docs/v0/14-UI独立发布.md)构建后，使用同一 `Activate` 校验/安装入口。
- 显式激活示例（需对应操作授权，UiRoot与实际taskd一致）：

  ```powershell
  .\distribution\windows\ui.ps1 -Action Activate -Package <原生包绝对路径> -UiRoot <实际安装目录/ui> -Url <实际taskd地址>
  ```

- 活动UI以 `/ui/status` 和首页固定版本资源为准。`/app.js` 返回内嵌兼容资源，不用于判断活动UI。当前二进制的embedded仍为React；`Rollback -Release embedded`会恢复React，不表示原生回退。

## 展示与只读边界

- 保留项目资料/来源、组件、目录资料、全部状态的关联任务及项目历史分页；任务项目归属保持紧凑，空组件为“未限定组件”。
- SourceRootView 为 id/projectId/componentId/directoryPath/createdAt，无 repositories/repositoryId/relativePath。目录纯资料，不验证或访问文件。移除 Worktree/代码现场/源码导航展示、API 调用及复制中的现场项。Checkpoint gitHead 仅作为历史资料保留。
- 七状态：backlog 暂不开始、todo 等待开始（默认）、in_progress 执行中、in_review 待审核或验收、blocked 受阻、done 已完成、cancelled 不再推进；支持全部七状态及 active/recent 筛选。
- 状态由用户或 AI 使用 `task status TASK STATUS --if-version VERSION` 任意转换。claim 不改变状态；Session 独立，无新 Session 动作或业务写入口。旧 close/block/unblock/pending-release/continue 命令不再使用。
- closureOutcome/closureReason/closedAt 仅展示历史值，不替代当前状态，不与 done/cancelled 联动。历史旧事件标签与原始正文保留，新增 task.status_changed 标签。
- 未提供、未填写、读取失败分别显示。GET失败支持重试，迟到响应不覆盖新选择。
- 任务目标/范围/验收、下一步/关闭说明、Note（含历史正文）、Checkpoint 统一按安全 Markdown 展示；原始文本及复制内容不变。支持标题、列表、引用、代码块、表格、http/https/mailto 链接；原始 HTML 显示为文本，不加载 Markdown 图片。代码块和表格局部横向滚动，读取失败不显示为空记录。
- Markdown 解析和样式源位于 `web/src/lib/markdown.ts`、`web/src/markdown.css`，与 React 共用。运行 `cd web && npm run build:native-markdown` 更新本目录 `app.js/style.css` 的生成前缀；不手改生成前缀，不改变三文件包合同，不自动部署或更新预览。
- `45`、`#45`精确匹配编号，仍受视图/项目筛选限制；普通文本继续匹配标题、目标和范围。正式后端已支持，旧后端不能仅靠UI发布启用。
- favicon由taskd同源 `/favicon.ico` 提供，本目录ICO作为二进制内嵌资源，不增加UI包文件；页面引用带内容摘要的版本参数，避免旧图标缓存。替换图标字节需要重新编译后端并同步引用摘要。
- 任务/项目概览展示有效通用和项目规则、revision、完整正文、依据及来源任务历史版本；来源链接读取任务当前详情，不冒充历史快照。正文使用textContent，不执行HTML。
- 缺失/未知格式/错误范围规则不当作空集合；规则不可用时拒绝复制不完整上下文。复制前重新读取最新context，包含项目资料、规则完整正文/来源、Checkpoint和备注。
- 项目刷新保留已加载范围、展开项及滚动；失败保留内容和可继续使用的游标。规则展开状态在刷新后保留。
- 传输层拒绝认证以外的POST；无规则或Task业务维护入口。规则不授予领取、推送、部署或关闭任务权限。

## 验证

```bash
fnm exec --using=24.11.1 node --check crates/server/web-legacy-readonly/app.js
cd web
# 静态合成页面：无服务、无数据库、无网络；检查 Markdown 及 1360/390/320px 布局。
fnm exec --using=24.11.1 node scripts/markdown-smoke.mjs
fnm exec --using=24.11.1 npm.cmd test -- src/lib/native-rules.test.ts src/lib/native-pagination.test.ts
fnm exec --using=24.11.1 npm.cmd run typecheck
cd ..
STEWARD_TEST_BIN_DIR='<兼容Schema8/合同5和task status的可信开发二进制目录>' \
  fnm exec --using=24.11.1 node web/scripts/legacy-readonly-smoke.mjs
```

- 前端单元测试 111 项、资源门禁测试 9 项、原生历史测试 5 项通过；TypeScript、React 构建、原生 JS 语法及 diff 空白检查通过。覆盖七状态筛选、合同5 GET/SSE、独立历史关闭/阻塞字段、Session、历史新旧事件、Markdown安全渲染、目录资料及无现场查询。
- 静态 Chrome 153 Markdown 检查通过（1360/390/320px，无网络或写请求），截图位于 `web/.artifacts/markdown-2026-09-16T09-38-14-382Z/`。
- 真实后端 smoke 仅使用显式指定的开发二进制和临时库，通过 CLI 创建合成夹具；不访问正式库，使用专属停止标记正常退出。
- Schema8/合同5原生端到端 smoke 通过：开发二进制 `E:/ai/agent-steward/target/debug`，绑定 `172.19.10.185`，Node24.11.1、Chrome153.0.8010.48。覆盖 reader/admin 只读、资料/目录/规则、历史与分页、重试/迟到响应、1360/390/320px、CSP及所有 SQLite 表摘要不变。结果与截图：`web/.artifacts/native-2026-09-16T09-42-41.784Z/`。
- 启动授权检查期间禁用登录表单，避免未完成的检查清空新输入；包含单元回归。专属测试服务均通过 shutdown 标记正常退出，进程核验无 `target/debug/taskd.exe` 残留。人工验收仍待确认。
- 构建不自动同步 embedded。仓库内嵌 React 三文件已显式同步合同5并重新编译，通过隔离浏览器 smoke；截图位于 `web/.artifacts/2026-09-16T08-43-15.699Z/`。内嵌源码同步和隔离测试不改变正式服务或既有预览。
