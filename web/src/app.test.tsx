import { StrictMode } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './app';
import { createQueryClient } from './lib/query-client';
import { useWorkspaceStore } from './stores/workspace';

const task = { id: 40, title: '隔离测试任务', status: 'open', version: 1, goal: 'fixture', scope: null, acceptanceCriteria: null, nextStep: null, projectId: null, componentIds: [] };
const project = { id: 1, name: '隔离测试项目', revision: 1, createdAt: '', updatedAt: '' };
const reply = (data: unknown) => new Response(JSON.stringify({ ok: true, data, warnings: [] }));
let client: ReturnType<typeof createQueryClient>;
beforeEach(() => { client = createQueryClient(); useWorkspaceStore.getState().reset(); window.history.replaceState(null, '', '/'); });
afterEach(() => { client.clear(); vi.unstubAllGlobals(); });
function mount(handler?: (path: string) => Response | Promise<Response> | undefined, strict = false) {
  const transport = vi.fn<typeof fetch>().mockImplementation(async (input) => {
    const path = String(input);
    const custom = handler?.(path); if (custom) return custom;
    if (path === '/api/access') return reply({ role: 'reader', local: false, projectManagement: true });
    if (path === '/api/events') return new Response(new ReadableStream());
    if (path.startsWith('/api/tasks?')) return reply({ tasks: [task], hasMore: false, nextCursor: null });
    if (path === '/api/tasks/40/context') return reply({ task, project: null });
    if (path === '/api/tasks/40/notes') return reply({ notes: [{ id: 1, noteType: 'progress', text: '可追溯的进展', createdAt: '' }] });
    if (path === '/api/tasks/40/history') return reply({ history: [{ sequence: 1, changeType: 'task.created', summary: '隔离创建记录', occurredAt: '' }] });
    if (path === '/api/sessions?taskId=40') return reply({ sessions: [{ id: 'fixture-session', startedAt: '' }] });
    if (path.startsWith('/api/projects?')) return reply({ projects: [project], hasMore: false, nextAfter: null });
    if (path === '/api/projects/1' || decodeURIComponent(path) === '/api/projects/隔离测试项目') return reply({ project });
    if (path.startsWith('/api/projects/1/history?')) return reply({ history: [], hasMore: false, nextAfter: null });
    if (path === '/api/projects/1/components') return reply({ project, components: [] });
    if (path === '/api/projects/1/sources') return reply({ project, sources: [] });
    throw new Error(`Unexpected request: ${path}`);
  });
  vi.stubGlobal('fetch', transport);
  const app = <QueryClientProvider client={client}><App /></QueryClientProvider>;
  render(strict ? <StrictMode>{app}</StrictMode> : app);
  return transport;
}
async function connect() {
  await screen.findByRole('button', { name: /#40/ });
}
it('shows pending release in list and detail without a task write entry', async () => {
  const pending = { ...task, status: 'pending_release' };
  const transport = mount(path => {
    if (path.startsWith('/api/tasks?')) return reply({ tasks: [pending], hasMore: false, nextCursor: null });
    if (path === '/api/tasks/40/context') return reply({ task: pending, project: null });
  });
  await connect();
  expect(screen.getAllByText('待上线').length).toBeGreaterThanOrEqual(2);
  fireEvent.click(screen.getByRole('button', { name: /#40/ }));
  await screen.findByText('fixture');
  expect(screen.getAllByText('待上线').length).toBeGreaterThanOrEqual(3);
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
  expect(screen.queryByRole('button', { name: /继续修改|关闭任务|标记待上线/ })).not.toBeInTheDocument();
});
it('exchanges a one-use link once under StrictMode and clears it before any request', async () => {
  const code = 'a'.repeat(64);
  window.history.replaceState(null, '', '/?fixture=1#connect=' + code);
  const transport = mount((path) => {
    expect(location.hash).toBe('');
    return path === '/api/connect' ? reply({}) : undefined;
  }, true);
  await connect();
  const writes = transport.mock.calls.filter(([, init]) => init?.method === 'POST');
  expect(writes).toHaveLength(1);
  expect(writes[0][0]).toBe('/api/connect');
  expect(writes[0][1]).toMatchObject({ headers: { 'X-Steward-Connect': code, 'X-Steward-CSRF': '1' }, body: '{}' });
  expect(location.search).toBe('?fixture=1');
  expect(document.body.textContent).not.toContain(code);
  expect(localStorage.length).toBe(0); expect(sessionStorage.length).toBe(0);
});
it('rejects invalid links without exchanging or retaining the code', async () => {
  window.history.replaceState(null, '', '/#connect=invalid');
  const transport = mount(undefined, true);
  await screen.findByRole('button', { name: '连接' });
  expect(location.hash).toBe('');
  expect(transport).not.toHaveBeenCalled();
  expect(screen.getByRole('alert')).toHaveTextContent('自动连接链接无效');
});
it('does not retry a rejected one-use link and permits manual cookie checking', async () => {
  window.history.replaceState(null, '', '/#connect=' + 'b'.repeat(64));
  const transport = mount(path => path === '/api/connect' ? new Response(JSON.stringify({ ok: false, error: { code: 'INVALID_CONNECT', message: 'expired' } }), { status: 403 }) : undefined, true);
  fireEvent.click(await screen.findByRole('button', { name: '连接' }));
  await connect();
  expect(transport.mock.calls.filter(([path]) => path === '/api/connect')).toHaveLength(1);
  expect(location.hash).toBe('');
});
it('holds an uncertain one-use exchange for GET verification without replay', async () => {
  window.history.replaceState(null, '', '/#connect=' + 'c'.repeat(64));
  const transport = mount(path => path === '/api/connect' ? Promise.reject(new Error('lost response')) : undefined, true);
  const verify = await screen.findByRole('button', { name: '核对当前授权' });
  expect(screen.getByRole('button', { name: '连接' })).toBeDisabled();
  fireEvent.click(verify); await connect();
  expect(transport.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(1);
  expect(location.hash).toBe('');
});
it('automatically restores existing cookie authorization with GET-only requests', async () => {
  const transport = mount();
  expect(screen.getByText('正在检查授权…')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '连接' })).not.toBeInTheDocument();
  await connect();
  fireEvent.click(screen.getByRole('button', { name: /#40/ }));
  await screen.findByText('fixture');
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
  expect(screen.queryByRole('button', { name: /新建|编辑|改名|关闭任务|保存/ })).not.toBeInTheDocument();
});
it('restores local authorization under StrictMode without showing the login form', async () => {
  const transport = mount((path) => path === '/api/access' ? reply({ role: 'admin', local: true, projectManagement: true }) : undefined, true);
  await connect();
  expect(screen.queryByRole('button', { name: '连接' })).not.toBeInTheDocument();
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
});
it('stays disconnected after an initial 401 and never loads business data', async () => {
  const transport = mount((path) => path === '/api/access' ? new Response('expired', { status: 401 }) : undefined);
  await screen.findByRole('button', { name: '连接' });
  expect(transport.mock.calls.map(([path]) => path)).toEqual(['/api/access']);
  expect(screen.queryByRole('button', { name: /#40/ })).not.toBeInTheDocument();
});
it('allows a manual GET retry after a failed initial authorization check', async () => {
  let reads = 0;
  const transport = mount((path) => path === '/api/access' && ++reads === 1 ? Promise.reject(new Error('offline')) : undefined);
  fireEvent.click(await screen.findByRole('button', { name: '连接' }));
  await connect();
  expect(reads).toBe(2);
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
});
it('ignores a late startup response after unmount and a new page load', async () => {
  let finish!: (response: Response) => void;
  mount((path) => path === '/api/access' ? new Promise<Response>((resolve) => { finish = resolve; }) : undefined);
  await waitFor(() => expect(finish).toBeDefined());
  cleanup();
  mount(); await connect();
  await act(async () => finish(new Response('expired', { status: 401 })));
  expect(screen.getByRole('button', { name: /#40/ })).toBeInTheDocument();
  expect(client.getQueryData(['access'])).toMatchObject({ role: 'reader' });
});
it('restores the original brand and task views without instructional banners', async () => {
  const transport = mount(); await connect();
  expect(screen.getByRole('link', { name: 'Agent Steward · 本地任务工作台' })).toHaveTextContent('S');
  const views = screen.getByLabelText('任务状态视图');
  expect([...views.querySelectorAll('button')].map(button => button.textContent)).toEqual(['未关闭', '进行中', '待上线', '有阻塞', '已关闭', '最近全部']);
  expect(screen.queryByText(/项目资料和任务由 AI/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Web 仅用于检索和展示/)).not.toBeInTheDocument();
  for (const [name, parameter] of [['进行中', 'view=in-progress'], ['待上线', 'view=pending-release'], ['有阻塞', 'view=blocked'], ['已关闭', 'status=closed'], ['最近全部', 'view=recent'], ['未关闭', 'view=active']]) {
    fireEvent.click(screen.getByRole('button', { name }));
    await waitFor(() => expect(screen.getByRole('button', { name: '刷新' })).not.toBeDisabled());
    expect(transport.mock.calls.some(([path]) => String(path).includes(parameter))).toBe(true);
  }
});
it('changing a search key removes previous rows while a replacement request is pending', async () => {
  let finish: (response: Response) => void = () => {};
  mount((path) => path.includes('query=next') ? new Promise<Response>((resolve) => { finish = resolve; }) : undefined);
  await connect();
  fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'next' } });
  fireEvent.click(screen.getByRole('button', { name: '搜索' }));
  await waitFor(() => expect(screen.queryByRole('button', { name: /#40/ })).not.toBeInTheDocument());
  await act(async () => finish(reply({ tasks: [], hasMore: false, nextCursor: null })));
  await screen.findByText('暂无任务');
});
it('rejects project details composed from inconsistent revisions', async () => {
  mount((path) => path === '/api/projects/1/components' ? reply({ project: { ...project, revision: 2 }, components: [{ id: 1, name: '不可展示的旧组件' }] }) : undefined);
  await connect();
  fireEvent.click(screen.getByRole('button', { name: '项目' }));
  fireEvent.click(await screen.findByRole('button', { name: /隔离测试项目/ }));
  await screen.findByRole('alert');
  expect(screen.getByRole('alert')).toHaveTextContent('项目在读取期间发生变化');
  expect(screen.queryByText('不可展示的旧组件')).not.toBeInTheDocument();
});
it('keeps logout single-flight and prevents reconnect until the response settles', async () => {
  let finish!: (response: Response) => void;
  const transport = mount((path) => path === '/api/logout' ? new Promise<Response>((resolve) => { finish = resolve; }) : undefined);
  await connect();
  const logout = screen.getByRole('button', { name: '退出连接' });
  fireEvent.click(logout); fireEvent.click(logout);
  await waitFor(() => expect(finish).toBeDefined());
  expect(logout).toBeDisabled();
  expect(transport.mock.calls.filter(([path]) => path === '/api/logout')).toHaveLength(1);
  await act(async () => finish(reply({})));
  await screen.findByRole('heading', { name: '连接只读工作台' });
  expect(transport.mock.calls.filter(([path]) => path === '/api/access')).toHaveLength(1);
  expect(screen.queryByRole('button', { name: /#40/ })).not.toBeInTheDocument();
});
it('clears task data when authorization expires', async () => {
  let expired = false;
  mount((path) => expired && path.startsWith('/api/tasks?') ? new Response('expired', { status: 401 }) : undefined);
  await connect();
  expired = true;
  fireEvent.click(screen.getByRole('button', { name: '刷新' }));
  await screen.findByRole('heading', { name: '连接只读工作台' });
  expect(screen.queryByRole('button', { name: /#40/ })).not.toBeInTheDocument();
  expect(client.getQueryData(['tasks', 'active', '', null])).toBeUndefined();
});
it('loads notes, sessions, history and unavailable worktree only through reads', async () => {
  const transport = mount(); await connect();
  fireEvent.click(screen.getByRole('button', { name: /#40/ })); await screen.findByText('fixture');
  fireEvent.click(screen.getByRole('button', { name: '进展备注' })); await screen.findByText('可追溯的进展');
  fireEvent.click(screen.getByRole('button', { name: 'Session' })); await screen.findByText('fixture-session');
  fireEvent.click(screen.getByRole('button', { name: '历史' })); await screen.findByText('隔离创建记录');
  fireEvent.click(screen.getByRole('button', { name: '代码现场' })); await screen.findByText('代码现场不可观察');
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
});
it('looks up a project by unique name and filters its tasks without changing membership', async () => {
  const transport = mount(); await connect();
  fireEvent.click(screen.getByRole('button', { name: '项目' }));
  await screen.findByRole('button', { name: /隔离测试项目/ });
  fireEvent.change(screen.getByRole('searchbox'), { target: { value: '隔离测试项目' } });
  fireEvent.submit(screen.getByRole('searchbox').closest('form')!);
  const row = await screen.findByRole('button', { name: /隔离测试项目/ });
  await waitFor(() => expect(row).not.toBeDisabled());
  fireEvent.click(row);
  const related = await screen.findByRole('button', { name: '查看关联任务' });
  await waitFor(() => expect(related).not.toBeDisabled()); fireEvent.click(related);
  await screen.findByRole('button', { name: /#40/ });
  expect(transport.mock.calls.some(([path]) => String(path).includes('project=%23%231'))).toBe(true);
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
});
it('holds uncertain authentication until an explicit read-only check', async () => {
  let accessReads = 0;
  const transport = mount((path) => {
    if (path === '/api/access' && ++accessReads === 1) return new Response('expired', { status: 401 });
    return path === '/api/login' ? Promise.reject(new Error('lost response')) : undefined;
  });
  await screen.findByRole('button', { name: '连接' });
  fireEvent.change(screen.getByLabelText('连接凭据（本机授权可留空）'), { target: { value: 'synthetic' } });
  fireEvent.click(screen.getByRole('button', { name: '连接' }));
  await screen.findByRole('button', { name: '核对当前授权' });
  expect(screen.getByRole('button', { name: '连接' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: '核对当前授权' }));
  await screen.findByRole('button', { name: /#40/ });
  expect(transport.mock.calls.filter(([path]) => path === '/api/login')).toHaveLength(1);
  expect(screen.queryByRole('button', { name: '核对当前授权' })).not.toBeInTheDocument();
});
it('clears project filters and caches when a backend withdraws the project capability', async () => {
  const transport = mount(); await connect();
  await act(async () => useWorkspaceStore.getState().viewProjectTasks(1));
  await waitFor(() => expect(transport.mock.calls.some(([path]) => String(path).includes('project=%23%231'))).toBe(true));
  const before = transport.mock.calls.length;
  await act(async () => { client.setQueryData(['access'], { role: 'reader', local: false }); });
  await waitFor(() => expect(useWorkspaceStore.getState().projectFilter).toBeNull());
  expect(screen.queryByRole('button', { name: '项目' })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole('button', { name: '刷新' })).not.toBeDisabled());
  fireEvent.click(screen.getByRole('button', { name: '刷新' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '刷新' })).not.toBeDisabled());
  expect(transport.mock.calls.slice(before).some(([path]) => String(path).includes('project='))).toBe(false);
});
it('copies a freshly read context and preserves a selectable fallback without business writes', async () => {
  let reads = 0;
  const transport = mount((path) => path === '/api/tasks/40/context' ? reply({ task: { ...task, version: ++reads }, notesSinceCheckpoint: [], notesTruncated: true }) : undefined);
  await connect(); fireEvent.click(screen.getByRole('button', { name: /#40/ })); await screen.findByText('fixture');
  fireEvent.click(screen.getByRole('button', { name: '复制上下文' }));
  const text = await screen.findByLabelText('上下文（可手工复制）');
  expect((text as HTMLTextAreaElement).value).toContain('version 2');
  expect(text).toHaveAttribute('readonly');
  expect(reads).toBe(2);
  expect(transport.mock.calls.every(([, options]) => options?.method === 'GET')).toBe(true);
});
