# 原生只读 UI

本目录为正式原生 HTML/CSS/JS 源码，不是 React 构建输出。保持已确认的原生布局，不覆盖 `../web`、`../web-readonly` 或不可变发布包。

## 兼容与发布

- API合同 **4**、包格式1；需要已兼容的taskd。本地真实库为Schema7；纯UI激活不回退数据库或重启后端。
- 正式地址：`http://172.19.10.185:51850`；外置UI版本 `native-readonly-20260910075151`，release `b74d4a62823b24aebeb83f63542ea1d39465b69ec992c5e6e4046879a738c2df`。
- 预览复用 `.local/local-preview`，地址 `http://172.19.10.185:60446`，代理正式合同4接口，**真实数据、GET-only**，不转发凭据。无需额外合成后端。
- 原生产生的包只能包含本目录 `index.html/app.js/style.css` 和按实际字节SHA生成的 `manifest.json`。正式包位置 `.local/favicon-cache-20260910075151`；不能只把旧包合同号改成4。
- `distribution/windows/ui.ps1 -Action Build` 当前读取React的 `web/dist`，**不要用它替代原生包**；原生包按[UI包合同](../../../docs/v0/14-UI独立发布.md)构建后，使用同一 `Activate` 校验/安装入口。
- 显式激活示例（需对应操作授权，UiRoot与实际taskd一致）：

  ```powershell
  .\distribution\windows\ui.ps1 -Action Activate -Package <原生包绝对路径> -UiRoot <实际安装目录/ui> -Url <实际taskd地址>
  ```

- 活动UI以 `/ui/status` 和首页固定版本资源为准。`/app.js` 返回内嵌兼容资源，不用于判断活动UI。当前二进制的embedded仍为React；`Rollback -Release embedded`会恢复React，不表示原生回退。

## 展示与只读边界

- 保留项目资料/来源、组件、源码登记、含已关闭任务的关联列表及项目历史分页；任务项目归属保持紧凑，空组件为“未限定组件”。
- 项目源码登记与任务实际Worktree分开；未提供、未填写、读取失败分别显示。GET失败支持重试，迟到响应不覆盖新选择。
- 展示待上线状态与筛选、对应历史状态变化，不新增业务按钮。
- `45`、`#45`精确匹配编号，仍受视图/项目筛选限制；普通文本继续匹配标题、目标和范围。正式后端已支持，旧后端不能仅靠UI发布启用。
- favicon由taskd同源 `/favicon.ico` 提供，本目录ICO作为二进制内嵌资源，不增加UI包文件；页面引用带内容摘要的版本参数，避免旧图标缓存。替换图标字节需要重新编译后端并同步引用摘要。
- 任务/项目概览展示有效通用和项目规则、revision、完整正文、依据及来源任务历史版本；来源链接读取任务当前详情，不冒充历史快照。正文使用textContent，不执行HTML。
- 缺失/未知格式/错误范围规则不当作空集合；规则不可用时拒绝复制不完整上下文。复制前重新读取最新context，包含项目资料、规则完整正文/来源、Checkpoint和备注。
- 项目刷新保留已加载范围、展开项及滚动；失败保留内容和可继续使用的游标。规则展开状态在刷新后保留。
- 传输层拒绝认证以外的POST；无规则或Task业务维护入口。规则不授予领取、推送、部署或关闭任务权限。

## 验证

```bash
fnm exec --using=24.11.1 node --check crates/server/web-legacy-readonly/app.js
fnm exec --using=24.11.1 node --test crates/server/tests/browser/legacy-history.test.cjs
cd web
fnm exec --using=24.11.1 npm.cmd test -- src/lib/native-rules.test.ts src/lib/native-pagination.test.ts
fnm exec --using=24.11.1 npm.cmd run typecheck
cd ..
STEWARD_TEST_BIN_DIR='<兼容Schema7/合同4的可信三件套绝对目录>' \
  fnm exec --using=24.11.1 node web/scripts/legacy-readonly-smoke.mjs
```

- 原生规则/分页DOM回归8项、历史5项及TypeScript通过。
- 真实数据预览检查任务#45、项目##1、历史及390px布局；无页面错误、横向溢出或非GET请求。
- 隔离Chrome152/Node24.11.1 smoke通过：待上线、有效规则及完整来源复制、缺规则拒绝复制、HTML不执行、资料/源码/组件/分页、错误重试/迟到响应、1360/390/320px、reader/admin只读、所有SQLite表摘要不变。先检查预览，再运行隔离smoke，不将自动化等同于用户验收。
- smoke通过真实CLI创建合成夹具，不访问正式库；使用专属停止标记正常退出。结果 `web/.artifacts/native-2026-09-10T07-07-35.476Z/`。
- 正式服务为Schema7/API4；编号搜索与context无权限告警已作API核验。Chrome确认带版本的favicon返回200并可解码。用户人工验收待确认。
