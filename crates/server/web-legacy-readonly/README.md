# 原生只读 UI 开发源（#45）

本目录是原生 HTML/CSS/JS 界面，不是 React 构建输出，不覆盖 `../web`、`../web-readonly` 或已发布的不可变包。

## 版本与发布边界

- 当前开发源使用 **API 合同 2**，需要配套合同 2 的 taskd；包格式仍为 1。
- 正式原生页面的发布记录为 `native-readonly-20260909103231`，release `6567425e3f2c17c8d643d918a228ba09f05fdaee2617cdcb0fb741610d6b966b`，地址 `http://172.19.10.185:51850`，使用合同 1。当前开发源不等于该发布快照。
- 当前开发源尚未发布；`.local/local-preview` 仍保留合同 1 的原发布快照。合同 2 候选不能直接替换它并连接合同 1 正式后端。
- 后端升级与配套 UI 采用需要另行授权；不得改写旧包清单、隐式重启、迁移数据库或发布。
- 正式启用版本以 `/ui/status` 和首页固定版本资源为准；`/app.js` 是内嵌兼容资源，不能用来判断活动 UI。

传输层拒绝认证之外的 POST，无业务维护入口。保留原生布局、认证、SSE、源码导航与复制上下文，不扩展数据模型或业务 API。

## 展示

- 项目：简介、架构/入口、开发验证、核实依据、资料 revision/时间及来源任务/历史版本；组件与项目源码登记；包含 closed 的关联任务与项目历史分页。
- 历史：展示已有 before/after；首次填写显示无旧值，缺少历史值时明确提示，原始 payload 可展开。来源任务链接打开当前详情，不冒充历史快照。
- 任务：所属项目名称和完整资料入口、可展开简介、所选组件名称/ID；空选择为“未限定组件”，与字段未返回分开。不在任务主体铺开项目架构/开发资料。
- 项目源码登记与任务实际 Worktree 分开。无项目、未填写、无组件/源码、字段不支持、读取失败分别显示；支持 GET 重试和迟到响应隔离。
- 项目实时刷新保留关联任务/维护历史已加载范围、展开项和滚动位置。所有刷新读取完成后统一替换内容；失败保留原内容及可继续使用的下一页游标。

## 隔离验证

使用 Node 24.11.1、`web/` 的依赖以及显式指定的可信开发二进制；不默认使用安装程序、正式 URL 或正式数据库。

```bash
# 仓库根：语法检查及合成 DOM 分页回归
fnm exec --using=24.11.1 node --check crates/server/web-legacy-readonly/app.js
cd web
fnm exec --using=24.11.1 npm.cmd test -- src/lib/native-pagination.test.ts
cd ..
STEWARD_TEST_BIN_DIR='E:/ai/agent-steward/.local/task45-fix-target/debug' \
  fnm exec --using=24.11.1 node web/scripts/legacy-readonly-smoke.mjs
```

浏览器脚本默认绑定 127.0.0.1；可通过 `STEWARD_TEST_BIND` 指定本机网卡 IPv4，仍使用随机端口和严格认证，不修改安全策略。默认使用已安装 Chrome；`STEWARD_BROWSER_CHANNEL=chromium` 使用预先安装的 Playwright Chromium，脚本不自动安装浏览器。

脚本在临时 SQLite/runtime/UI 包中通过 CLI 准备合成数据，验证真实分页、closed 跳转、资料前后值、组件范围、空/缺字段/失败重试、迟到响应、1360/390/320px 布局、reader/admin 无业务 POST、CSP，以及所有 SQLite 表内容摘要不变。通过专属 shutdown marker 正常退出；失败保留沙箱。截图在 `web/.artifacts/native-*/`。

当前候选的合成 DOM 分页回归 3 项通过；Chrome 152/Node 24.11.1 原生浏览器 smoke 通过，使用 `.local/task45-fix-target/debug` 的 Schema5/API 合同 2 开发二进制。旧 `crates/server/tests/browser/*.test.cjs` 多数测试读取 `../web`，不代替当前原生候选验证。自动化通过不代表正式部署或人工验收。

发布只使用本目录的 `index.html/app.js/style.css`，遵循 [UI 独立发布合同](../../../docs/v0/14-UI独立发布.md)。不要将 README 加入包，也不要用 React 的 `sync:embedded` 代替原生候选发布。
