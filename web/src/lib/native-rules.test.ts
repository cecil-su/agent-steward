import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';

const source = fs.readFileSync('../crates/server/web-legacy-readonly/app.js', 'utf8');
const html = fs.readFileSync('../crates/server/web-legacy-readonly/index.html', 'utf8');
afterEach(() => { document.body.replaceChildren(); vi.restoreAllMocks(); });
const rule = (id = 1) => ({ id, revision: 2, status: 'active', scope: 'global', projectId: null, contentVersion: 1, content: { name: '长期偏好', body: '<img src=x onerror=alert(1)>\n完整规则正文', sources: [{ kind: 'inferred', evidence: '<script>unsafe()</script>\n反馈依据', taskId: 2, taskVersion: 3 }] } });
const snapshot = () => ({ task: { id: 45, title: '原生任务', status: 'in_review', version: 27, projectId: 1, componentIds: [], goal: '目标' }, project: { id: 1, name: '项目', revision: 1 }, projectProfile: { summary: '项目简介', sourceTaskId: 2, sourceTaskVersion: 3 }, sessionRules: { formatVersion: 1, rules: [rule()] }, checkpoint: null });
function fixture(notes: unknown[] = [], noteFailure = false, accessFailure = false) {
  document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
  Object.defineProperty(document, 'scrollingElement', { configurable: true, value: document.documentElement });
  let responseContext = snapshot();
  const transport = vi.fn(async (path: string, _options?: RequestInit) => {
    const u = new URL(path, 'http://synthetic.local');
    let data: unknown;
    if (/\/tasks\/\d+\/context$/.test(u.pathname)) data = responseContext;
    else if (u.pathname.endsWith('/notes')) {
      if (noteFailure) throw new Error('synthetic note read failure');
      data = { notes };
    }
    else if (u.pathname === '/api/projects/1') data = { project: responseContext.project, profile: responseContext.projectProfile, sessionRules: responseContext.sessionRules };
    else if (u.pathname.endsWith('/components')) data = { project: responseContext.project, components: [] };
    else if (u.pathname.endsWith('/sources')) data = { project: responseContext.project, sources: [{ id: 3, projectId: 1, componentId: null, directoryPath: '/unverified/docs', createdAt: '2026-09-16' }] };
    else if (u.pathname.endsWith('/history')) data = { history: [], hasMore: false };
    else if (u.pathname === '/api/tasks') data = { tasks: [], hasMore: false };
    else if (u.pathname === '/api/access') {
      if (accessFailure) throw new Error('synthetic access failure');
      data = { projectManagement: true, sessionRules: true };
    }
    else throw new Error(path);
    return { ok: true, status: 200, json: async () => ({ ok: true, data, warnings: [] }) };
  });
  const writeText = vi.fn(async (_text: string) => {});
  const end = source.lastIndexOf('  startUiUpdates();');
  const code = source.slice(0, end).replace('(() => {', 'return (() => {') + `connected=true;projectsSupported=true;projectPageVisible=true;return {api,connect,renderDetail,selectProject,copyContext,contextText,taskHistoryItem,statuses,setContext(value){context=value;selected=value.task.id;}};})();`;
  const api = new Function('document', 'fetch', 'navigator', code)(document, transport, { clipboard: { writeText } });
  api.setContext(responseContext);
  return { ...api, transport, writeText, response: (value: unknown) => { responseContext = value as ReturnType<typeof snapshot>; } };
}

it('disables credential entry until the initial authorization check settles', async () => {
  const f = fixture([], false, true);
  const pending = f.connect();
  for (const control of document.querySelectorAll('#connect-form input, #connect-form button')) expect(control).toBeDisabled();
  await pending;
  for (const control of document.querySelectorAll('#connect-form input, #connect-form button')) expect(control).not.toBeDisabled();
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => options.method === 'GET')).toBe(true);
});

it('keeps native layout, filters all seven statuses and uses contract5 for reads and SSE', async () => {
  const f = fixture(); await f.renderDetail();
  expect(document.querySelector('#detail .badge')).toHaveTextContent('待审核或验收');
  for (const [view, parameter] of [['backlog','status=backlog'],['todo','status=todo'],['in-progress','view=in-progress'],['in-review','view=in-review'],['blocked','view=blocked'],['done','status=done'],['cancelled','status=cancelled'],['recent','view=recent'],['active','view=active']]) {
    await (document.querySelector<HTMLButtonElement>(`[data-view="${view}"]`)!.onclick as unknown as () => Promise<void>)();
    expect(f.transport.mock.calls.some(([path]: [string]) => path.includes(parameter))).toBe(true);
  }
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => (options.headers as Record<string, string>)['X-Steward-UI-Contract'] === '5')).toBe(true);
  expect(source).toContain("fetch('/api/events',{headers:{'X-Steward-UI-Contract':'5'}");
  await expect(f.api('/api/commands/task-create', {})).rejects.toThrow('当前界面只读');
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => options.method === 'GET')).toBe(true);
});

it('renders complete rules and historical sources as text in task and project panels', async () => {
  const f = fixture(); await f.renderDetail();
  let panel = document.querySelector('#detail .session-rules')!;
  expect(panel).toHaveTextContent('完整规则正文');
  expect(panel).toHaveTextContent('反馈依据');
  expect(panel).toHaveTextContent('记录时任务版本 3（历史引用，不代表当前版本）');
  expect(panel.querySelector('img,script')).toBeNull();
  const detail = panel.querySelector<HTMLDetailsElement>('details')!; detail.open = true;
  await f.renderDetail(); expect(document.querySelector<HTMLDetailsElement>('#detail .session-rules details')!.open).toBe(true);
  await f.selectProject(1);
  panel = document.querySelector('#project-detail .session-rules')!;
  expect(panel).toHaveTextContent('完整规则正文');
  expect(panel.querySelector('button')).toHaveTextContent('来源任务 #2');
  expect(panel.querySelector('img,script')).toBeNull();
});

it('distinguishes no rules from missing/unsupported/inapplicable data and blocks incomplete copying', async () => {
  const f = fixture();
  for (const rules of [undefined, { formatVersion: 2, rules: [] }, { formatVersion: 1, rules: [{ ...rule(), status: 'candidate' }] }, { formatVersion: 1, rules: [{ ...rule(), scope: 'project', projectId: 2 }] }, { formatVersion: 1, rules: [rule(), rule()] }]) {
    f.setContext({ ...snapshot(), sessionRules: rules }); await f.renderDetail();
    expect(document.querySelector('#detail .session-rules')).toHaveTextContent('有效规则不可用');
    expect(() => f.contextText()).toThrow('有效规则不可用');
  }
  f.setContext({ ...snapshot(), sessionRules: { formatVersion: 1, rules: [] } }); await f.renderDetail();
  expect(document.querySelector('#detail .session-rules')).toHaveTextContent('暂无有效规则');
  expect(f.contextText()).toContain('"rules": []');
});

it('copy re-reads the latest snapshot and includes complete rules and project provenance', async () => {
  const f = fixture(); const current = snapshot(); current.sessionRules.rules[0].revision = 3;
  current.sessionRules.rules[0].content.body = '最新完整正文'; f.response(current);
  await f.copyContext();
  expect(f.writeText).toHaveBeenCalledTimes(1);
  const copied = f.writeText.mock.calls[0][0];
  for (const text of ['待审核或验收', '最新完整正文', '"revision": 3', '"taskVersion": 3', '项目简介', 'sourceTaskVersion', '不构成执行授权', '未限定组件']) expect(copied).toContain(text);
});

it('renders Markdown in done task descriptions, checkpoint, notes and note history without writes', async () => {
  const note = { id: 8, noteType: 'progress', text: '## 备注标题\n\n- **备注内容**\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n<img src=x onerror=alert(1)>', createdAt: '' };
  const f = fixture([note]);
  f.setContext({ ...snapshot(), task: { ...snapshot().task, status: 'done', goal: '**目标内容**', scope: '**范围内容**', acceptanceCriteria: '**验收内容**', closureOutcome: 'completed' }, checkpoint: { summary: '**摘要**', completed: ['**完成项**'], decisions: [], pending: [], risks: [], nextStep: '**后续说明**' } });
  await f.renderDetail();
  const root = document.querySelector('#detail')!;
  for (const text of ['目标内容', '范围内容', '验收内容', '摘要', '完成项', '后续说明', '备注内容']) {
    expect(Array.from(root.querySelectorAll('strong'), n => n.textContent)).toContain(text);
  }
  expect(root.querySelector('.steward-markdown table')).not.toBeNull();
  expect(root.querySelector('.steward-markdown img')).toBeNull();
  const row = f.taskHistoryItem({ sequence: 9, changeType: 'task.noted', payload: { noteId: 8 } }, {}, [note], false, new Set());
  expect(row.querySelector('h2')).toHaveTextContent('备注标题');
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => options.method === 'GET')).toBe(true);
});

it('does not turn a note read failure into an empty note list', async () => {
  const f = fixture([], true); await f.renderDetail();
  expect(document.querySelector('#detail')).toHaveTextContent('任务备注读取失败');
  expect(document.querySelector('#detail')).not.toHaveTextContent('暂无记录');
});

it('pending-release history uses Chinese labels and before/after status', () => {
  const f = fixture();
  const row = f.taskHistoryItem({ sequence: 1, changeType: 'task.pending_release', payload: { previousStatus: 'in_progress', status: 'pending_release' } }, {}, [], false, new Set(['task-history-changes-1']));
  expect(row).toHaveTextContent('开发结束，待上线');
  expect(row).toHaveTextContent('修改前：进行中'); expect(row).toHaveTextContent('修改后：待上线');
  expect(row.querySelector('pre')!.textContent).toBe(JSON.stringify({ previousStatus: 'in_progress', status: 'pending_release' }, null, 2));
});

it('labels status changes without rewriting historical payloads', () => {
  const f = fixture();
  const payload = { previousStatus: 'done', status: 'in_progress' };
  const row = f.taskHistoryItem({ sequence: 2, changeType: 'task.status_changed', payload }, {}, [], false, new Set());
  expect(row).toHaveTextContent('变更任务状态');
  expect(row).toHaveTextContent('修改前：已完成');
  expect(row).toHaveTextContent('修改后：执行中');
  expect(row.querySelector('pre')!.textContent).toBe(JSON.stringify(payload, null, 2));
});

it.each(['in_progress', 'done'])('keeps migrated blocking records historical after blocked → %s, including verbatim copy', async (targetStatus) => {
  const f = fixture();
  const blockReason = '  **旧阻塞原因**\n第二行 <script>原文</script>  ';
  const blockRecovery = '  旧恢复条件\n- 保留空白与 Markdown  ';
  for (const status of ['blocked', targetStatus]) {
    const current = { ...snapshot(), task: { ...snapshot().task, status, blockReason, blockRecovery } };
    const original = JSON.stringify(current);
    f.setContext(current); f.response(current);
    await f.copyContext();
    const root = document.querySelector('#detail')!;
    expect(root.querySelector('.badge')).toHaveTextContent(f.statuses[status]);
    const legacy = Array.from(root.querySelectorAll('section')).find(section => section.firstElementChild?.textContent === '历史阻塞记录')!;
    expect(legacy).toHaveTextContent('不代表当前状态');
    expect(Array.from(legacy.querySelectorAll('p'), p => p.textContent)).toContain(blockReason);
    expect(Array.from(legacy.querySelectorAll('p'), p => p.textContent)).toContain(blockRecovery);
    expect(legacy.querySelector('script')).toBeNull();
    const copied = f.writeText.mock.calls.at(-1)![0];
    expect(copied).toContain(`状态：${f.statuses[status]}`);
    expect(copied).toContain(`## 历史阻塞记录（不代表当前状态）\n历史阻塞原因：\n${blockReason}\n历史恢复条件：\n${blockRecovery}\n`);
    expect(copied).not.toContain('\n## 阻塞\n');
    expect(JSON.stringify(current)).toBe(original);
  }
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => options.method === 'GET')).toBe(true);
});

it.each([undefined, null, ''])('includes recovery-only history when reason is %s without inventing a reason', async (blockReason) => {
  const f = fixture();
  const blockRecovery = '  仅有历史恢复条件\n原文尾部  ';
  f.setContext({ ...snapshot(), task: { ...snapshot().task, status: 'done', blockReason, blockRecovery } });
  await f.renderDetail();
  const root = document.querySelector('#detail')!;
  expect(root).toHaveTextContent('历史阻塞记录');
  expect(root).toHaveTextContent('不代表当前状态');
  expect(root).toHaveTextContent('历史恢复条件');
  expect(root.textContent).not.toContain('历史阻塞原因');
  const copied = f.contextText();
  expect(copied).toContain(`## 历史阻塞记录（不代表当前状态）\n历史恢复条件：\n${blockRecovery}\n`);
  expect(copied).not.toContain('历史阻塞原因');
});

it.each([{}, { blockReason: null, blockRecovery: null }, { blockReason: '', blockRecovery: '' }])('omits blocking history when no historical values exist: %j', async (fields) => {
  const f = fixture();
  f.setContext({ ...snapshot(), task: { ...snapshot().task, ...fields } });
  await f.renderDetail();
  expect(document.querySelector('#detail')!.textContent).not.toMatch(/历史阻塞|历史恢复/);
  expect(f.contextText()).not.toMatch(/历史阻塞|历史恢复|\n## 阻塞\n/);
});

it('keeps closure history and sessions independent of every current status and removes live-source reads', async () => {
  const f = fixture();
  for (const status of ['backlog', 'todo', 'in_progress', 'in_review', 'blocked', 'done', 'cancelled']) {
    f.setContext({ ...snapshot(), task: { ...snapshot().task, status, nextStep: '**下一步原文**', closureOutcome: 'partial', closureReason: '**历史原因**', closedAt: '2025-01-01', worktreePath: '/old-worktree' }, session: { id: 'unchanged-session' }, worktreeStatus: { head: 'old-live-head' } });
    await f.renderDetail();
    const root = document.querySelector('#detail')!;
    expect(root.querySelector('.badge')).toHaveTextContent(f.statuses[status]);
    expect(root).toHaveTextContent('历史关闭记录');
    expect(root).toHaveTextContent('历史原因');
    expect(root).toHaveTextContent('下一步原文');
    expect(root.textContent).not.toContain('代码现场');
    const copied = f.contextText();
    expect(copied).toContain('unchanged-session');
    expect(copied).not.toMatch(/old-live-head|old-worktree|实时 Git/);
  }
  await f.selectProject(1);
  expect(document.querySelector('#project-detail')).toHaveTextContent('/unverified/docs');
  expect(document.querySelector('#project-detail')).toHaveTextContent('不验证存在性');
  expect(document.querySelector('#project-detail')!.textContent).not.toMatch(/查看源码上下文|Repository|common-dir/);
  expect(f.transport.mock.calls.every(([path, options]: [string, RequestInit]) => options.method === 'GET' && !/worktree|\/projects\/\d+\/context/.test(path))).toBe(true);
});
