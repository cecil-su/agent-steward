import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';

const source = fs.readFileSync('../crates/server/web-legacy-readonly/app.js', 'utf8');
const html = fs.readFileSync('../crates/server/web-legacy-readonly/index.html', 'utf8');
afterEach(() => { document.body.replaceChildren(); vi.restoreAllMocks(); });
const rule = (id = 1) => ({ id, revision: 2, status: 'active', scope: 'global', projectId: null, contentVersion: 1, content: { name: '长期偏好', body: '<img src=x onerror=alert(1)>\n完整规则正文', sources: [{ kind: 'inferred', evidence: '<script>unsafe()</script>\n反馈依据', taskId: 2, taskVersion: 3 }] } });
const snapshot = () => ({ task: { id: 45, title: '原生任务', status: 'pending_release', version: 27, projectId: 1, componentIds: [], goal: '目标' }, project: { id: 1, name: '项目', revision: 1 }, projectProfile: { summary: '项目简介', sourceTaskId: 2, sourceTaskVersion: 3 }, sessionRules: { formatVersion: 1, rules: [rule()] }, checkpoint: null });
function fixture() {
  document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
  Object.defineProperty(document, 'scrollingElement', { configurable: true, value: document.documentElement });
  let responseContext = snapshot();
  const transport = vi.fn(async (path: string, _options?: RequestInit) => {
    const u = new URL(path, 'http://synthetic.local');
    let data: unknown;
    if (/\/tasks\/\d+\/context$/.test(u.pathname)) data = responseContext;
    else if (u.pathname.endsWith('/notes')) data = { notes: [] };
    else if (u.pathname === '/api/projects/1') data = { project: responseContext.project, profile: responseContext.projectProfile, sessionRules: responseContext.sessionRules };
    else if (u.pathname.endsWith('/components')) data = { project: responseContext.project, components: [] };
    else if (u.pathname.endsWith('/sources')) data = { project: responseContext.project, sources: [], repositories: [] };
    else if (u.pathname.endsWith('/history')) data = { history: [], hasMore: false };
    else if (u.pathname === '/api/tasks') data = { tasks: [], hasMore: false };
    else if (u.pathname === '/api/access') data = { projectManagement: true, sessionRules: true };
    else throw new Error(path);
    return { ok: true, status: 200, json: async () => ({ ok: true, data, warnings: [] }) };
  });
  const writeText = vi.fn(async (_text: string) => {});
  const end = source.lastIndexOf('  startUiUpdates();');
  const code = source.slice(0, end).replace('(() => {', 'return (() => {') + `connected=true;projectsSupported=true;projectPageVisible=true;return {api,renderDetail,selectProject,copyContext,contextText,taskHistoryItem,statuses,setContext(value){context=value;selected=value.task.id;}};})();`;
  const api = new Function('document', 'fetch', 'navigator', code)(document, transport, { clipboard: { writeText } });
  api.setContext(responseContext);
  return { ...api, transport, writeText, response: (value: unknown) => { responseContext = value as ReturnType<typeof snapshot>; } };
}

it('keeps the native layout, adds pending-release filter and uses contract4 for reads and SSE', async () => {
  const f = fixture(); await f.renderDetail();
  expect(document.querySelector('#detail .badge')).toHaveTextContent('待上线');
  expect(document.querySelector('[data-view="pending-release"]')).toHaveTextContent('待上线');
  await (document.querySelector<HTMLButtonElement>('[data-view="pending-release"]')!.onclick as unknown as () => Promise<void>)();
  expect(f.transport.mock.calls.some(([path]: [string]) => path.includes('view=pending-release'))).toBe(true);
  expect(f.transport.mock.calls.every(([, options]: [string, RequestInit]) => (options.headers as Record<string, string>)['X-Steward-UI-Contract'] === '4')).toBe(true);
  expect(source).toContain("fetch('/api/events',{headers:{'X-Steward-UI-Contract':'4'}");
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
  for (const text of ['待上线', '最新完整正文', '"revision": 3', '"taskVersion": 3', '项目简介', 'sourceTaskVersion', '不构成执行授权', '未限定组件']) expect(copied).toContain(text);
});

it('pending-release history uses Chinese labels and before/after status', () => {
  const f = fixture();
  const row = f.taskHistoryItem({ sequence: 1, changeType: 'task.pending_release', payload: { previousStatus: 'in_progress', status: 'pending_release' } }, {}, [], false, new Set(['task-history-changes-1']));
  expect(row).toHaveTextContent('开发结束，待上线');
  expect(row).toHaveTextContent('修改前：进行中'); expect(row).toHaveTextContent('修改后：待上线');
});
