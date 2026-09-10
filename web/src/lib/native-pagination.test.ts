import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';

// Execute the actual native candidate against a DOM and synthetic GET responses.
const source = fs.readFileSync('../crates/server/web-legacy-readonly/app.js', 'utf8');
const html = fs.readFileSync('../crates/server/web-legacy-readonly/index.html', 'utf8');
let stop: (() => void) | undefined;
afterEach(() => { stop?.(); document.body.replaceChildren(); vi.useRealTimers(); });
function fixture() {
  document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
  Object.defineProperty(document, 'scrollingElement', { configurable: true, value: document.documentElement });
  const project = { id: 1, name: 'Synthetic project', revision: 1 };
  let fail = false;
  let hold: (() => Promise<void>) | undefined;
  const cursors: number[] = [];
  const transport = vi.fn(async (path: string) => {
    const u = new URL(path, 'http://fixture.local');
    let data: unknown;
    if (u.pathname === '/api/projects') data = { projects: [project], hasMore: false };
    else if (/^\/api\/projects\/\d+$/.test(u.pathname)) data = { project: { ...project, id: Number(u.pathname.split('/').at(-1)) }, profile: null };
    else if (u.pathname.endsWith('/components')) data = { project, components: [] };
    else if (u.pathname.endsWith('/sources')) data = { project, sources: [], repositories: [] };
    else if (u.pathname.endsWith('/history')) {
      const after = Number(u.searchParams.get('after')); cursors.push(after);
      if (hold) await hold();
      if (fail && after === 2) throw new Error('synthetic page failure');
      data = { history: [after + 1, after + 2].map(revision => ({ revision, occurredAt: '', changeType: 'project.created', payload: { name: 'fixture' } })), hasMore: after < 4, nextAfter: after + 2 };
    } else if (u.pathname === '/api/tasks') {
      const after = Number(u.searchParams.get('cursor'));
      data = { tasks: [after + 1, after + 2].map(id => ({ id, title: 'Task ' + id, status: 'closed' })), hasMore: after < 4, nextCursor: String(after + 2) };
    } else throw new Error(path);
    return { ok: true, status: 200, json: async () => ({ ok: true, data, warnings: [] }) };
  });
  const end = source.lastIndexOf('  startUiUpdates();');
  const code = source.slice(0, end).replace('(() => {', 'return (() => {') + 'connected=true;projectsSupported=true;projectPageVisible=true;return {selectProject,scheduleLiveRefresh,stopLive};})();';
  const api = new Function('document', 'fetch', code)(document, transport) as { selectProject: (id: number, refresh?: boolean) => Promise<void>; scheduleLiveRefresh: () => void; stopLive: () => void };
  stop = api.stopLive;
  const area = (key: string) => document.querySelector<HTMLElement>(`#project-detail [data-project-page="${key}"]`)!;
  const more = async (key: string) => {
    const b = area(key).querySelector<HTMLButtonElement>('button:not(.text-link)')!;
    await (b.onclick as unknown as () => Promise<void>)();
  };
  return { ...api, area, more, cursors, fail: (value: boolean) => { fail = value; }, hold: (value?: () => Promise<void>) => { hold = value; } };
}
it('retains loaded task/history ranges, disclosures and scroll through an SSE refresh', async () => {
  vi.useFakeTimers(); const f = fixture();
  await f.selectProject(1); await f.more('tasks'); await f.more('history');
  document.querySelector<HTMLDetailsElement>('[data-detail-key="project-history-3"]')!.open = true;
  document.documentElement.scrollTop = 200;
  f.scheduleLiveRefresh(); await vi.advanceTimersByTimeAsync(300);
  expect(f.area('tasks').dataset.loadedCount).toBe('4');
  expect(f.area('history').dataset.loadedCount).toBe('4');
  expect(f.cursors).toEqual([0, 2, 0, 2]);
  expect(document.querySelector<HTMLDetailsElement>('[data-detail-key="project-history-3"]')!.open).toBe(true);
  expect(document.documentElement.scrollTop).toBe(200);
  await f.more('history'); expect(f.area('history').dataset.loadedCount).toBe('6');
});
it('keeps the whole visible range and usable next cursor when a later refresh page fails', async () => {
  const f = fixture(); await f.selectProject(1); await f.more('tasks'); await f.more('history');
  const previous = f.area('history'); f.fail(true);
  await f.selectProject(1, true);
  expect(f.area('history')).toBe(previous);
  expect(f.area('tasks').dataset.loadedCount).toBe('4');
  expect(document.getElementById('notice')).toHaveTextContent('保留原有内容');
  f.fail(false); await f.more('history');
  expect(f.area('history').dataset.loadedCount).toBe('6');
  expect(f.cursors.at(-1)).toBe(4);
});
it('does not publish an old background project response after navigation', async () => {
  const f = fixture(); await f.selectProject(1); await f.more('history');
  let finish!: () => void;
  f.hold(() => new Promise<void>(resolve => { finish = resolve; }));
  const pending = f.selectProject(1, true);
  await vi.waitFor(() => expect(finish).toBeDefined());
  f.hold(); await f.selectProject(2);
  finish(); await pending;
  expect(document.querySelector('#project-detail .detail-heading')).toHaveTextContent('##2');
  expect(f.area('history').dataset.loadedCount).toBe('2');
});
