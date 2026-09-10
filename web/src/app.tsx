import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { Button } from './components/ui/button';
import { Input } from './components/ui/input';
import { Label } from './components/ui/label';
import { ReadonlyWorkspace } from './features/workspace';
import { useLiveUpdates } from './hooks/use-live-updates';
import { useUiRelease } from './hooks/use-ui-release';
import { ApiError, createApi } from './lib/api';
import { formatTaskContext } from './lib/context';
import type { Access, Component, HistoryEntry, Note, Project, ProjectDetail, ProjectHistoryPage, ProjectPage, Session, Source, TaskContext, TaskPage } from './lib/contracts';
import { useWorkspaceStore } from './stores/workspace';

export function App() {
  const client = useQueryClient();
  const ui = useWorkspaceStore();
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(true);
  const [checkingInitialAccess, setCheckingInitialAccess] = useState(true);
  const [uncertainAuth, setUncertainAuth] = useState(false);
  const authBusy = useRef(false);
  const generation = useRef(0);
  const [credential, setCredential] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [copy, setCopy] = useState<{ id: number; text: string } | null>(null);
  const [copying, setCopying] = useState(false);
  const unauthorized = useCallback(() => {
    generation.current++;
    setMessage('浏览器授权已失效，请重新连接。');
    setConnected(false); setCredential(''); setWarnings([]); setCopy(null); setUncertainAuth(false);
    client.clear(); useWorkspaceStore.getState().reset();
  }, [client]);
  const api = useMemo(() => createApi({
    getGeneration: () => generation.current,
    onUnauthorized: unauthorized,
    onWarnings: (items) => { if (items.length) setWarnings(items.map((item) => `${item.code}：${item.message}`)); },
  }), [unauthorized]);
  // Once per page load: reuse local authorization or the existing HttpOnly cookie.
  // Never replay login, and do not rerun this effect after an explicit logout.
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    const currentGeneration = ++generation.current;
    authBusy.current = true;
    void (async () => {
      // StrictMode's setup/cleanup probe must not consume or replay a one-use POST.
      await Promise.resolve();
      if (!active) return;
      const code = new URLSearchParams(location.hash.slice(1)).get('connect');
      if (code !== null) {
        window.history.replaceState(null, '', location.pathname + location.search);
        if (!/^[a-f0-9]{64}$/.test(code)) throw new Error('自动连接链接无效，请手动连接。');
        await api.connect(code, controller.signal);
      }
      return api.get<Access>('/api/access', controller.signal);
    })().then((result) => {
      if (!active || currentGeneration !== generation.current || !result) return;
      client.setQueryData(['access'], result);
      setConnected(true);
    }).catch((error: unknown) => {
      if (active && currentGeneration === generation.current) {
        setMessage(error instanceof Error ? error.message : '无法检查授权，请手动连接。');
        if (error instanceof ApiError && error.uncertain) setUncertainAuth(true);
      }
    }).finally(() => {
      if (active) {
        authBusy.current = false;
        setConnecting(false);
        setCheckingInitialAccess(false);
      }
    });
    return () => { active = false; controller.abort(); generation.current++; authBusy.current = false; };
  }, [api, client]);
  const access = useQuery({ queryKey: ['access'], queryFn: ({ signal }) => api.get<Access>('/api/access', signal), enabled: connected, staleTime: Infinity });
  const projectFilter = access.data?.projectManagement === true ? ui.projectFilter : null;
  useEffect(() => {
    if (!connected || !access.data || access.data.projectManagement === true) return;
    useWorkspaceStore.getState().disableProjects();
    client.removeQueries({ predicate: (query) => String(query.queryKey[0]).startsWith('project') });
    setCopy(null);
  }, [connected, access.data?.projectManagement, client]);
  const tasks = useInfiniteQuery({
    queryKey: ['tasks', ui.view, ui.query, projectFilter], initialPageParam: null as string | null,
    queryFn: ({ pageParam, signal }) => {
      const params = new URLSearchParams(ui.view === 'closed' ? { status: 'closed', pageSize: '30' } : { view: ui.view, pageSize: '30' });
      if (ui.query) params.set('query', ui.query);
      if (projectFilter !== null && client.getQueryData<Access>(['access'])?.projectManagement === true) params.set('project', `##${projectFilter}`);
      if (pageParam) params.set('cursor', pageParam);
      return api.get<TaskPage>(`/api/tasks?${params}`, signal);
    },
    getNextPageParam: (last) => last.hasMore ? last.nextCursor : undefined,
    enabled: connected && ui.tab === 'tasks',
  });
  const projects = useInfiniteQuery({
    queryKey: ['projects'], initialPageParam: 0,
    queryFn: ({ pageParam, signal }) => api.get<ProjectPage>(`/api/projects?after=${pageParam}&limit=50`, signal),
    getNextPageParam: (last) => last.hasMore ? last.nextAfter : undefined,
    enabled: connected && ui.tab === 'projects' && !ui.projectQuery && access.data?.projectManagement === true,
  });
  const projectLookup = useQuery({
    queryKey: ['project-lookup', ui.projectQuery],
    queryFn: ({ signal }) => api.get<{ project: Project }>(`/api/projects/${encodeURIComponent(ui.projectQuery)}`, signal),
    enabled: connected && ui.tab === 'projects' && !!ui.projectQuery && access.data?.projectManagement === true,
  });
  const task = useQuery({
    queryKey: ['task', ui.selectedTaskId], queryFn: ({ signal }) => api.get<TaskContext>(`/api/tasks/${ui.selectedTaskId}/context`, signal),
    enabled: connected && ui.tab === 'tasks' && ui.selectedTaskId !== null,
  });
  const notes = useQuery({
    queryKey: ['notes', ui.selectedTaskId], queryFn: ({ signal }) => api.get<{ notes: Note[] }>(`/api/tasks/${ui.selectedTaskId}/notes`, signal),
    enabled: connected && ui.tab === 'tasks' && ui.selectedTaskId !== null && ui.detailTab === 'notes',
  });
  const sessions = useQuery({
    queryKey: ['sessions', ui.selectedTaskId], queryFn: ({ signal }) => api.get<{ sessions: Session[] }>(`/api/sessions?taskId=${ui.selectedTaskId}`, signal),
    enabled: connected && ui.tab === 'tasks' && ui.selectedTaskId !== null && ui.detailTab === 'sessions',
  });
  const history = useQuery({
    queryKey: ['history', ui.selectedTaskId], queryFn: ({ signal }) => api.get<{ history: HistoryEntry[] }>(`/api/tasks/${ui.selectedTaskId}/history`, signal),
    enabled: connected && ui.tab === 'tasks' && ui.selectedTaskId !== null && ui.detailTab === 'history',
  });
  const project = useQuery({
    queryKey: ['project', ui.selectedProjectId],
    queryFn: async ({ signal }): Promise<ProjectDetail> => {
      const path = `/api/projects/${ui.selectedProjectId}`;
      const [info, components, sources] = await Promise.all([
        api.get<Pick<ProjectDetail, 'project' | 'profile' | 'sessionRules'>>(path, signal),
        api.get<{ project: Project; components: Component[] }>(`${path}/components`, signal),
        api.get<{ project: Project; sources: Source[] }>(`${path}/sources`, signal),
      ]);
      if (info.project.revision !== components.project.revision || info.project.revision !== sources.project.revision) throw new Error('项目在读取期间发生变化，请刷新后重试。');
      return { project: info.project, profile: info.profile, sessionRules: info.sessionRules, components: components.components, sources: sources.sources };
    },
    enabled: connected && ui.tab === 'projects' && access.data?.projectManagement === true && ui.selectedProjectId !== null,
  });
  const projectHistory = useInfiniteQuery({
    queryKey: ['project-history', ui.selectedProjectId], initialPageParam: 0,
    queryFn: ({ pageParam, signal }) => api.get<ProjectHistoryPage>(`/api/projects/${ui.selectedProjectId}/history?after=${pageParam}&limit=50`, signal),
    getNextPageParam: (last) => last.hasMore ? last.nextAfter : undefined,
    enabled: connected && ui.tab === 'projects' && access.data?.projectManagement === true && ui.selectedProjectId !== null,
  });
  const protectedInput = uncertainAuth || !!credential || copying || !!copy || ui.searchInput.trim() !== ui.query || ui.projectSearchInput.trim() !== ui.projectQuery;
  const refresh = useCallback(async () => {
    await client.invalidateQueries({ predicate: (query) => query.queryKey[0] !== 'access', refetchType: 'active' }, { cancelRefetch: false });
  }, [client]);
  const liveStatus = useLiveUpdates(connected && !connecting, {
    protectedInput,
    refresh,
    unauthorized,
    revalidate: async () => { const result = await api.get<Access>('/api/access'); client.setQueryData(['access'], result); },
  });
  const uiRelease = useUiRelease(protectedInput, connecting || uncertainAuth);
  const connect = async () => {
    if (authBusy.current || uncertainAuth) return;
    authBusy.current = true;
    const currentGeneration = ++generation.current;
    const token = credential.trim();
    setCredential(''); setMessage(null); setWarnings([]); setConnecting(true);
    try {
      if (token) await api.login(token);
      await client.fetchQuery({ queryKey: ['access'], queryFn: ({ signal }) => api.get<Access>('/api/access', signal), staleTime: 0 });
      if (currentGeneration === generation.current) setConnected(true);
    } catch (error) { if (currentGeneration === generation.current) { setMessage(error instanceof Error ? error.message : '连接失败'); if (error instanceof ApiError && error.uncertain) setUncertainAuth(true); } }
    finally { authBusy.current = false; setConnecting(false); }
  };
  const disconnect = async () => {
    if (authBusy.current || uncertainAuth) return;
    authBusy.current = true; setConnecting(true);
    const currentGeneration = ++generation.current;
    await client.cancelQueries();
    try {
      await api.logout();
      if (currentGeneration !== generation.current) return;
      generation.current++;
      setConnected(false); setMessage(null); setWarnings([]); setCopy(null); client.clear(); ui.reset();
    } catch (error) { if (currentGeneration === generation.current) { setMessage(error instanceof Error ? error.message : '退出结果未确认，请核对。'); if (error instanceof ApiError && error.uncertain) setUncertainAuth(true); } }
    finally { authBusy.current = false; setConnecting(false); }
  };
  const verifyAuth = async () => {
    if (authBusy.current) return;
    authBusy.current = true; setConnecting(true);
    const currentGeneration = generation.current;
    try {
      const result = await api.get<Access>('/api/access');
      if (currentGeneration !== generation.current) return;
      client.setQueryData(['access'], result);
      setUncertainAuth(false); setConnected(true); setMessage(null);
    } catch (error) { if (currentGeneration === generation.current) setMessage(error instanceof Error ? error.message : '当前授权仍无法确认'); }
    finally { authBusy.current = false; setConnecting(false); }
  };
  const copyContext = async () => {
    const id = ui.selectedTaskId;
    if (id === null || copying) return;
    const currentGeneration = generation.current;
    setCopying(true); setMessage(null);
    try {
      const fresh = await api.get<TaskContext>(`/api/tasks/${id}/context`);
      if (currentGeneration !== generation.current || useWorkspaceStore.getState().selectedTaskId !== id) return;
      const text = formatTaskContext(fresh);
      // Always retain an explicit, selectable fallback for HTTP/blocked clipboards.
      setCopy({ id, text });
      try { await navigator.clipboard?.writeText(text); } catch { /* Manual copy remains available. */ }
    } catch (error) { if (currentGeneration === generation.current) setMessage(error instanceof Error ? error.message : '读取上下文失败'); }
    finally { setCopying(false); }
  };
  const clearCopy = () => { setCopy(null); setMessage(null); };
  const currentList = ui.tab === 'tasks' ? tasks : ui.projectQuery ? projectLookup : projects;
  const currentDetail = ui.tab === 'tasks' ? task : project;
  const extra = ui.tab === 'tasks' ? ({ notes, sessions, history }[ui.detailTab as 'notes' | 'sessions' | 'history']) : projectHistory;
  const error = message ?? currentList.error?.message ?? currentDetail.error?.message ?? extra?.error?.message ?? null;
  const busy = connecting || copying || currentList.isFetching || currentDetail.isFetching || !!extra?.isFetching;
  return (
    <div className="min-h-screen">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-card px-5 py-4 md:px-10">
        <a href="/" aria-label="Agent Steward · 本地任务工作台" className="flex items-center gap-3 text-foreground no-underline">
          <span aria-hidden="true" className="brand-mark">S</span>
          <span><strong className="block">Agent Steward</strong><small className="block text-[10px] tracking-wider text-muted-foreground">本地任务工作台</small></span>
        </a>
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-xs text-muted-foreground">只读</span>
          {connected && <span role="status" className="text-xs text-muted-foreground">{liveStatus}</span>}
          {connected && <Button variant="outline" disabled={connecting || uncertainAuth} onClick={() => { void disconnect(); }}>退出连接</Button>}
        </div>
      </header>
      <main className="mx-auto max-w-[1440px] space-y-5 px-4 py-6 md:px-10">
        {uncertainAuth && <aside role="status" className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm">认证结果未确认；不会自动重试或采用新版。<Button variant="outline" disabled={connecting} onClick={() => { void verifyAuth(); }}>核对当前授权</Button></aside>}
        {uiRelease.available && <aside role="status" className="rounded-lg border border-border bg-card p-4 text-sm">新版 UI 已就绪，当前输入已保留。<Button className="ml-3" variant="outline" disabled={connecting || uncertainAuth} onClick={() => uiRelease.apply()}>刷新采用新版</Button></aside>}
        {checkingInitialAccess ? <p role="status" className="text-sm text-muted-foreground">正在检查授权…</p> : !connected ? (
          <form className="mx-auto max-w-md rounded-xl border border-border bg-card p-6" onSubmit={(event) => { event.preventDefault(); void connect(); }}>
            <h1 className="text-xl font-semibold">连接只读工作台</h1>
            <Label htmlFor="credential">连接凭据（本机授权可留空）</Label>
            <Input id="credential" type="password" autoComplete="off" value={credential} onChange={(event) => setCredential(event.target.value)} disabled={connecting} />
            <p className="my-3 text-xs text-muted-foreground">凭据不持久化，使用同源 HttpOnly Cookie。本机授权可直接连接；其他设备使用只读凭据。</p>
            <Button type="submit" disabled={connecting || uncertainAuth}>{connecting ? '正在连接…' : '连接'}</Button>
            {message && <p role="alert" className="mt-3 text-sm text-destructive">{message}</p>}
          </form>
        ) : <>
          {!!warnings.length && <div role="status" className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm">{warnings.map((warning) => <p key={warning}>{warning}</p>)}</div>}
          {copy && <Button variant="outline" onClick={() => setCopy(null)}>关闭复制预览</Button>}
          <ReadonlyWorkspace tab={ui.tab} onTabChange={(tab) => { clearCopy(); ui.setTab(tab); }} tasks={tasks.data?.pages.flatMap((page) => page.tasks) ?? []}
            projects={ui.projectQuery ? (projectLookup.data ? [projectLookup.data.project] : []) : projects.data?.pages.flatMap((page) => page.projects) ?? []}
            taskContext={task.data} projectDetail={project.data} selectedTaskId={ui.selectedTaskId} selectedProjectId={ui.selectedProjectId}
            onSelectTask={(id) => { clearCopy(); ui.selectTask(id); }} onSelectProject={(id) => { clearCopy(); ui.selectProject(id); }} projectManagement={access.data?.projectManagement === true}
            busy={busy} error={error} hasMore={ui.tab === 'tasks' ? tasks.hasNextPage : !ui.projectQuery && projects.hasNextPage}
            onMore={() => { if (!busy) void (ui.tab === 'tasks' ? tasks.fetchNextPage() : projects.fetchNextPage()); }}
            onRefresh={() => { setMessage(null); void refresh(); }}
            query={ui.tab === 'tasks' ? ui.searchInput : ui.projectSearchInput}
            onQueryChange={ui.tab === 'tasks' ? ui.setSearchInput : ui.setProjectSearchInput}
            onSearch={() => { clearCopy(); if (ui.tab === 'tasks') { ui.applySearch(); if (ui.searchInput.trim() === ui.query) void tasks.refetch(); } else { ui.applyProjectSearch(); if (ui.projectSearchInput.trim() === ui.projectQuery) void (ui.projectQuery ? projectLookup.refetch() : projects.refetch()); } }}
            view={ui.view} onViewChange={(view) => { clearCopy(); ui.setView(view); }} detailTab={ui.detailTab} onDetailTabChange={(tab) => { clearCopy(); ui.setDetailTab(tab); }}
            notes={notes.data?.notes} sessions={sessions.data?.sessions} history={history.data?.history}
            projectHistory={projectHistory.data?.pages.flatMap((page) => page.history)} projectHistoryHasMore={projectHistory.hasNextPage}
            onMoreProjectHistory={() => { if (!busy) void projectHistory.fetchNextPage(); }}
            onCopyContext={() => { void copyContext(); }} copyText={copy?.id === ui.selectedTaskId && ui.tab === 'tasks' ? copy.text : null}
            projectFilter={projectFilter} onClearProjectFilter={() => { clearCopy(); ui.clearProjectFilter(); }}
            onViewProjectTasks={(id) => { clearCopy(); ui.viewProjectTasks(id); }} onOpenTaskProject={(id) => { clearCopy(); ui.openTaskProject(id); }} />
        </>}
      </main>
    </div>
  );
}
