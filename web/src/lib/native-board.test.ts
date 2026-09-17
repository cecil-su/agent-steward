import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';

const source = fs.readFileSync('../crates/server/web-legacy-readonly/app.js', 'utf8');
const html = fs.readFileSync('../crates/server/web-legacy-readonly/index.html', 'utf8');
afterEach(() => { document.body.replaceChildren(); window.onpopstate = null; window.history.replaceState(null, '', '/'); vi.useRealTimers(); vi.restoreAllMocks(); });
function fixture(admin = false, pathname = '/') {
  window.history.replaceState(null, '', pathname);
  document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
  Object.defineProperty(document, 'scrollingElement', { configurable: true, value: document.documentElement });
  let task = { id: 1, version: 2, status: 'todo', title: '合成看板卡片', currentSessionId: 'unchanged' };
  let outcome = 'ok';
  const transport = vi.fn(async (path: string, options: RequestInit) => {
    const url = new URL(path, 'http://fixture');
    if (options.method === 'POST') {
      if (outcome === 'uncertain') throw new Error('synthetic connection loss');
      if (outcome === 'conflict') return { ok: false, status: 409, json: async () => ({ ok: false, error: { code: 'VERSION_CONFLICT', message: 'changed' } }) };
      const body = JSON.parse(String(options.body));
      task = { ...task, status: body.status, version: task.version + 1 };
      return { ok: true, status: 200, json: async () => ({ ok: true, data: { task } }) };
    }
    const status = url.searchParams.get('status');
    const tasks = status === task.status ? [task] : [];
    return { ok: true, status: 200, json: async () => ({ ok: true, data: { tasks, hasMore: false, nextCursor: null } }) };
  });
  const end = source.lastIndexOf('  startUiUpdates();');
  const code = source.slice(0, end).replace('(() => {', 'return (() => {') + `connected=true;canChangeStatus=${admin};return {changeLayout,loadList,moreBoard,moveStatus,scheduleLiveRefresh,api,setProjectPage(){projectPageVisible=true;},setQuery(value){query=value;},getTask(){return board.todo?.tasks[0];}};})();`;
  const api = new Function('document', 'fetch', code)(document, transport);
  return { ...api, transport, writes: () => transport.mock.calls.filter(([, o]) => o.method === 'POST'), outcome: (value: string) => { outcome = value; } };
}
it('initializes the full-page dashboard from its route and uses real route links', async () => {
  const f = fixture(false, '/dashboard'); await f.loadList();
  expect(document.body).toHaveClass('dashboard-page');
  expect(document.querySelector('#board-mode')).toHaveAttribute('href', '/dashboard');
  expect(document.querySelector('#list-mode')).toHaveAttribute('href', '/');
  expect(document.querySelector('#board-mode')).toHaveAttribute('aria-current', 'page');
  expect(document.querySelectorAll('.kanban-column')).toHaveLength(7);
  expect(document.body).not.toHaveClass('dashboard-detail-open');
  await f.changeLayout('list');
  expect(location.pathname).toBe('/');
  expect(document.body).not.toHaveClass('dashboard-page');
});
it('loads seven individually filtered columns, keeps readers non-draggable and all other mutations disabled', async () => {
  const f = fixture(); await f.changeLayout('board');
  expect(document.querySelectorAll('.kanban-column')).toHaveLength(7);
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'false');
  expect(document.querySelector('#status-views')).toHaveAttribute('hidden');
  await f.moveStatus(f.getTask(), 'done');
  await expect(f.api('/api/commands/task-status', {})).rejects.toThrow('当前界面只读');
  expect(f.writes()).toHaveLength(0);
  expect(f.transport.mock.calls.map(([p]: [string]) => new URL(p, 'http://fixture').searchParams.get('status'))).toEqual(['backlog','todo','in_progress','in_review','blocked','done','cancelled']);
});
it('bounds column read concurrency instead of exhausting server slots', async () => {
  const f = fixture(), base = f.transport.getMockImplementation();
  let active = 0, peak = 0;
  f.transport.mockImplementation(async (path: string, options: RequestInit) => {
    active++; peak = Math.max(peak, active);
    try { await new Promise(resolve => setTimeout(resolve, 1)); return await base(path, options); }
    finally { active--; }
  });
  await f.changeLayout('board');
  expect(peak).toBe(1);
  expect(document.querySelectorAll('.kanban-column')).toHaveLength(7);
});
it('sends only the captured version and target status with CSRF/contract headers, without Session fields', async () => {
  const f = fixture(true); await f.changeLayout('board');
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'true');
  await f.moveStatus(f.getTask(), 'done');
  expect(f.writes()).toHaveLength(1);
  const [path, options] = f.writes()[0];
  expect(path).toBe('/api/commands/task-status');
  expect(JSON.parse(String(options.body))).toEqual({ taskId: 1, expectedVersion: 2, status: 'done' });
  expect(options.headers).toMatchObject({ 'X-Steward-CSRF': '1', 'X-Steward-UI-Contract': '5' });
  expect(document.querySelector('[data-status="done"]')).toHaveTextContent('合成看板卡片');
  expect(document.querySelector('[data-status="todo"] .task-card')).toBeNull();
  await expect(f.api('/api/commands/task-create', {})).rejects.toThrow('当前界面只读');
});
it.each(['conflict', 'uncertain'])('holds the original card after %s until an explicit read, with no retry', async outcome => {
  const f = fixture(true); await f.changeLayout('board'); const original = f.getTask();
  f.outcome(outcome); await f.moveStatus(original, 'done'); await f.moveStatus(original, 'cancelled');
  expect(f.writes()).toHaveLength(1);
  expect(document.querySelector('[data-status="todo"]')).toHaveTextContent('合成看板卡片');
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'false');
  expect(document.querySelector('#notice')).toHaveTextContent('刷新');
  await f.loadList();
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'true');
  expect(f.writes()).toHaveLength(1);
});
it('does not post same-column drops or accept stale/unlisted cards', async () => {
  const f = fixture(true); await f.changeLayout('board');
  await f.moveStatus(f.getTask(), 'todo');
  await f.moveStatus({ ...f.getTask(), version: 1 }, 'done');
  await f.moveStatus({ ...f.getTask(), id: 77 }, 'done');
  expect(f.writes()).toHaveLength(0);
});
it('serializes status writes and holds layout while the request is pending', async () => {
  const f = fixture(true); await f.changeLayout('board'); const original = f.getTask();
  const base = f.transport.getMockImplementation();
  let finish!: () => void;
  const gate = new Promise<void>(resolve => { finish = resolve; });
  f.transport.mockImplementation(async (path: string, options: RequestInit) => {
    if (options.method === 'POST') await gate;
    return base(path, options);
  });
  const pending = f.moveStatus(original, 'done');
  await f.moveStatus(original, 'cancelled'); await f.changeLayout('list');
  expect(document.querySelector('#board-mode')).toHaveAttribute('aria-current', 'page');
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'false');
  expect(f.writes()).toHaveLength(1);
  finish(); await pending;
  expect(f.writes()).toHaveLength(1);
});
it('rejects a partial column refresh without mixing old search results or enabling writes', async () => {
  const f = fixture(true); await f.changeLayout('board');
  const base = f.transport.getMockImplementation();
  f.transport.mockImplementation((path: string, options: RequestInit) => {
    if (new URL(path, 'http://fixture').searchParams.get('status') === 'blocked') throw new Error('synthetic read failure');
    return base(path, options);
  });
  f.setQuery('new scope');
  await expect(f.loadList()).rejects.toThrow('无法读取');
  expect(document.querySelectorAll('.kanban .task-card')).toHaveLength(0);
  expect(f.writes()).toHaveLength(0);
});
it('discards a late column page after refresh and restores current card interactivity', async () => {
  const f = fixture(true), base = f.transport.getMockImplementation();
  let finish!: () => void;
  const gate = new Promise<void>(resolve => { finish = resolve; });
  f.transport.mockImplementation(async (path: string, options: RequestInit) => {
    const url = new URL(path, 'http://fixture');
    if (url.searchParams.get('status') !== 'todo') return base(path, options);
    const more = url.searchParams.has('cursor');
    if (more) await gate;
    const tasks = Array.from({ length: more ? 1 : 30 }, (_, i) => ({ id: more ? 31 : i + 1, version: 2, status: 'todo', title: '合成分页任务' }));
    return { ok: true, status: 200, json: async () => ({ ok: true, data: { tasks, hasMore: !more, nextCursor: more ? null : 'next' } }) };
  });
  await f.changeLayout('board'); const pending = f.moreBoard('todo');
  await f.loadList(); finish(); await pending;
  expect(document.querySelectorAll('[data-status="todo"] .task-card')).toHaveLength(30);
  expect(document.querySelector('[data-task-id="31"]')).toBeNull();
  expect(document.querySelector('.task-card')).toHaveAttribute('draggable', 'true');
});
it('removes the old board immediately on list navigation, even if the list read fails', async () => {
  const f = fixture(true); await f.changeLayout('board'); const oldCard = f.getTask();
  let fail!: () => void;
  f.transport.mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = () => reject(new Error('synthetic list failure')); }));
  const pending = f.changeLayout('list').catch((error: Error) => error);
  try {
    expect(location.pathname).toBe('/');
    expect(document.querySelector('.kanban')).toBeNull();
    await f.moveStatus(oldCard, 'done'); expect(f.writes()).toHaveLength(0);
  } finally {
    fail(); expect((await pending).message).toContain('无法读取');
  }
  expect(document.querySelector('.kanban')).toBeNull();
});
it('continues readonly list SSE refresh after a board conflict without retrying the write', async () => {
  vi.useFakeTimers();
  const f = fixture(true); await f.changeLayout('board');
  f.outcome('conflict'); await f.moveStatus(f.getTask(), 'done');
  await f.changeLayout('list'); const before = f.transport.mock.calls.length;
  f.scheduleLiveRefresh(); await vi.advanceTimersByTimeAsync(250);
  expect(f.transport.mock.calls.length).toBeGreaterThan(before);
  expect(f.writes()).toHaveLength(1);
});
it('keeps project SSE reads available while the board still requires explicit reconciliation', async () => {
  vi.useFakeTimers();
  const f = fixture(true); await f.changeLayout('board');
  f.outcome('conflict'); await f.moveStatus(f.getTask(), 'done'); f.setProjectPage();
  f.transport.mockImplementationOnce(async () => ({ ok: true, status: 200, json: async () => ({ ok: true, data: { projects: [], hasMore: false, nextAfter: null } }) }));
  const before = f.transport.mock.calls.length;
  f.scheduleLiveRefresh(); await vi.advanceTimersByTimeAsync(250);
  expect(f.transport.mock.calls.slice(before).map(([path]: [string]) => path)).toEqual(['/api/projects?after=0&limit=50']);
  expect(f.writes()).toHaveLength(1);
});
it('does not label unread columns as empty when the first board load fails', async () => {
  const f = fixture(true);
  f.transport.mockImplementationOnce(async () => { throw new Error('synthetic board failure'); });
  await expect(f.changeLayout('board')).rejects.toThrow('无法读取');
  expect(document.querySelector('.kanban')).not.toHaveTextContent('暂无任务');
  expect(document.querySelector('.kanban')).toHaveTextContent('未读取');
  expect(document.querySelectorAll('[draggable="true"]')).toHaveLength(0);
});
it('scopes every column to the current search and can return to the existing list', async () => {
  const f = fixture(); f.setQuery('合成'); await f.changeLayout('board');
  expect(f.transport.mock.calls.every(([p]: [string]) => new URL(p, 'http://fixture').searchParams.get('query') === '合成')).toBe(true);
  await f.changeLayout('list');
  expect(document.querySelector('#status-views')).not.toHaveAttribute('hidden');
  expect(document.querySelector('.kanban')).toBeNull();
});
