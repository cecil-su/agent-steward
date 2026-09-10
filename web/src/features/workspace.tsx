import { useId } from 'react';
import type { Task, Project, TaskContext, ProjectDetail, DetailTab, Note, Session, HistoryEntry } from '../lib/contracts';
import { HistoryPanel, ProjectProfilePanel, TaskDetailPanel } from './detail-panels';
import { Textarea } from '../components/ui/textarea';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';
import { EmptyState } from '../components/ui/empty-state';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { PageHeading } from '../components/ui/page-heading';

export interface ReadonlyWorkspaceProps {
  detailTab?: DetailTab;
  onDetailTabChange?: (tab: DetailTab) => void;
  notes?: Note[];
  sessions?: Session[];
  history?: HistoryEntry[];
  projectHistory?: HistoryEntry[];
  projectHistoryHasMore?: boolean;
  onMoreProjectHistory?: () => void;
  onCopyContext?: () => void;
  copyText?: string | null;
  projectFilter?: number | null;
  onClearProjectFilter?: () => void;
  onViewProjectTasks?: (id: number) => void;
  onOpenTaskProject?: (id: number) => void;
  tab: 'tasks' | 'projects';
  onTabChange: (tab: 'tasks' | 'projects') => void;
  tasks: Task[];
  projects: Project[];
  taskContext?: TaskContext;
  projectDetail?: ProjectDetail;
  selectedTaskId: number | null;
  selectedProjectId: number | null;
  onSelectTask: (id: number) => void;
  onSelectProject: (id: number) => void;
  projectManagement: boolean;
  busy: boolean;
  error: string | null;
  hasMore: boolean;
  onMore: () => void;
  onRefresh: () => void;
  query: string;
  onQueryChange: (query: string) => void;
  onSearch: () => void;
  view: string;
  onViewChange: (view: string) => void;
}

const views = [
  ['active', '未关闭'],
  ['in-progress', '进行中'],
  ['pending-release', '待上线'],
  ['blocked', '有阻塞'],
  ['closed', '已关闭'],
  ['recent', '最近全部'],
] as const;
const statusLabels = { open: '待处理', in_progress: '进行中', pending_release: '待上线', blocked: '受阻', closed: '已关闭' };

function TaskBadge({ task }: { task: Task }) {
  return <Badge variant={task.status === 'open' ? 'default' : task.status}>{statusLabels[task.status]}</Badge>;
}

function Field({ label, value }: { label: string; value: string | number | null | undefined }) {
  return (
    <div className="space-y-1">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="m-0 whitespace-pre-wrap break-words text-sm">{value ?? '未设置'}</dd>
    </div>
  );
}

export function ReadonlyWorkspace(props: ReadonlyWorkspaceProps) {
  const searchId = useId();
  const copyId = useId();
  const detailTab = props.detailTab ?? 'overview';
  const isTasks = props.tab === 'tasks';
  // Never show detail left over from a previously selected item.
  const task = props.taskContext?.task.id === props.selectedTaskId ? props.taskContext.task : undefined;
  const detail = props.projectDetail?.project.id === props.selectedProjectId ? props.projectDetail : undefined;

  return (
    <div className="bg-background text-foreground">
      <div>
        <nav aria-label="工作区" className="mb-6 flex flex-wrap gap-2">
          <Button variant={isTasks ? 'default' : 'outline'} aria-pressed={isTasks} disabled={props.busy} onClick={() => props.onTabChange('tasks')}>任务</Button>
          {props.projectManagement && (
            <Button variant={!isTasks ? 'default' : 'outline'} aria-pressed={!isTasks} disabled={props.busy} onClick={() => props.onTabChange('projects')}>项目</Button>
          )}
        </nav>
        <PageHeading
          title={isTasks ? '任务' : '项目'}
          description={isTasks ? '查看任务进展、会话记录和下一步。' : '查看项目资料和关联任务。'}
          actions={
            <>
              {!isTasks && <Button size="default" variant="outline" disabled={props.busy} onClick={() => props.onTabChange('tasks')}>返回任务</Button>}
              <Button size="default" className="min-w-24 disabled:opacity-100" variant="outline" disabled={props.busy} onClick={props.onRefresh}><span role={props.busy ? 'status' : undefined}>{props.busy ? '正在加载…' : '刷新'}</span></Button>
            </>
          }
        />
        {isTasks && props.projectFilter != null && (
          <div className="mb-5 flex flex-wrap items-center gap-3 rounded-lg border border-border bg-secondary p-3 text-sm">
            <span>当前项目筛选：##{props.projectFilter}</span>
            {props.onClearProjectFilter && <Button variant="outline" disabled={props.busy} onClick={props.onClearProjectFilter}>清除项目筛选</Button>}
          </div>
        )}
        {props.error && <div role="alert" className="mb-5 whitespace-pre-wrap break-words rounded-lg border border-[#e6c6bf] bg-[#fbeeea] p-4 text-sm text-destructive">{props.error}</div>}
        {!isTasks && !props.projectManagement ? (
          <Card><CardContent className="p-6"><EmptyState title="项目浏览不可用" description="当前环境未启用项目管理。" /></CardContent></Card>
        ) : (
          <>
            {isTasks && (
              <div aria-label="任务状态视图" className="mb-5 flex flex-wrap gap-2">
                {views.map(([value, label]) => (
                  <Button key={value} className="disabled:opacity-100" variant={props.view === value ? 'secondary' : 'ghost'} aria-pressed={props.view === value} disabled={props.busy} onClick={() => props.onViewChange(value)}>{label}</Button>
                ))}
              </div>
            )}
            <div aria-busy={props.busy} className="grid items-start gap-6 lg:grid-cols-[320px_minmax(0,1fr)]">
              <Card className="h-[340px] min-w-0 overflow-y-auto lg:h-[520px]">
                <CardHeader><CardTitle>{isTasks ? '任务列表' : '项目列表'}</CardTitle></CardHeader>
                <CardContent className="space-y-4">
                    <form role="search" className="space-y-2" onSubmit={(event) => { event.preventDefault(); if (!props.busy) props.onSearch(); }}>
                      <Label htmlFor={searchId}>{isTasks ? '搜索任务' : '查找项目（唯一名称或 ##编号）'}</Label>
                      <div className="flex gap-2">
                        <Input id={searchId} type="search" value={props.query} disabled={props.busy} onChange={(event) => props.onQueryChange(event.target.value)} placeholder={isTasks ? '输入任务关键词' : '输入唯一名称或 ##编号'} />
                        <Button type="submit" variant="outline" disabled={props.busy}>搜索</Button>
                      </div>
                      {!isTasks && <p className="text-xs text-muted-foreground">按唯一名称或 ##编号查找，不是模糊全库搜索。</p>}
                    </form>
                  {isTasks ? (
                    props.tasks.length ? <ul className="m-0 list-none space-y-2 p-0">{props.tasks.map((item) => (
                      <li key={item.id}>
                        <button type="button" aria-pressed={props.selectedTaskId === item.id} disabled={props.busy} onClick={() => props.onSelectTask(item.id)} className="w-full rounded-lg border border-border bg-card p-4 text-left hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 aria-pressed:border-primary aria-pressed:bg-secondary">
                          <span className="mb-2 flex flex-wrap items-center justify-between gap-2"><span className="text-xs text-muted-foreground">#{item.id}</span><TaskBadge task={item} /></span>
                          <span className="block break-words text-sm font-semibold">{item.title || '未命名任务'}</span>
                        </button>
                      </li>
                    ))}</ul> : <EmptyState className="px-4 py-12" title={props.busy ? '正在加载任务' : '暂无任务'} description="可调整视图或搜索条件后重试。" />
                  ) : (
                    props.projects.length ? <ul className="m-0 list-none space-y-2 p-0">{props.projects.map((item) => (
                      <li key={item.id}>
                        <button type="button" aria-pressed={props.selectedProjectId === item.id} disabled={props.busy} onClick={() => props.onSelectProject(item.id)} className="w-full rounded-lg border border-border bg-card p-4 text-left hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 aria-pressed:border-primary aria-pressed:bg-secondary">
                          <span className="block text-xs text-muted-foreground">##{item.id} · 修订 {item.revision}</span>
                          <span className="mt-2 block break-words text-sm font-semibold">{item.name}</span>
                        </button>
                      </li>
                    ))}</ul> : <EmptyState className="px-4 py-12" title={props.busy ? '正在加载项目' : '暂无项目'} description="此处展示可浏览的项目。" />
                  )}
                  {props.hasMore && <Button variant="outline" className="w-full" disabled={props.busy} onClick={props.onMore}>加载更多</Button>}
                </CardContent>
              </Card>
              <Card className="min-h-[360px] min-w-0 lg:min-h-[520px]">
                <CardHeader><CardTitle>{isTasks ? '任务详情' : '项目详情'}</CardTitle></CardHeader>
                <CardContent>
                  {isTasks ? task ? (
                    <div className="space-y-5">
                      <div className="flex flex-wrap items-center gap-2"><Badge variant="outline">#{task.id}</Badge><TaskBadge task={task} /></div>
                      <h3 className="break-words text-xl font-semibold">{task.title || '未命名任务'}</h3>
                      <div className="flex flex-wrap gap-2" aria-label="任务详情导航">
                        {([['overview', '概览'], ['notes', '进展备注'], ['sessions', 'Session'], ['worktree', '代码现场'], ['history', '历史']] as const).map(([value, label]) => (
                          <Button key={value} variant={detailTab === value ? 'secondary' : 'ghost'} aria-pressed={detailTab === value} disabled={props.busy || !props.onDetailTabChange} onClick={() => props.onDetailTabChange?.(value)}>{label}</Button>
                        ))}
                        {props.onCopyContext && <Button variant="outline" disabled={props.busy} onClick={props.onCopyContext}>复制上下文</Button>}
                      </div>
                      {props.copyText != null && <div className="space-y-2"><Label htmlFor={copyId}>上下文（可手工复制）</Label><Textarea id={copyId} value={props.copyText} readOnly rows={8} /></div>}
                      {detailTab === 'overview' && <dl className="m-0 space-y-4">
                        <Field label="版本" value={task.version} />
                        <Field label="目标" value={task.goal} />
                        <Field label="范围" value={task.scope} />
                        <Field label="验收标准" value={task.acceptanceCriteria} />
                        <Field label="下一步" value={task.nextStep} />
                        <Field label="项目" value={props.taskContext?.project && props.taskContext.project.id === task.projectId ? props.taskContext.project.name : task.projectId} />
                        <Field label="组件 ID" value={(task.componentIds ?? []).join('、') || '无'} />
                      </dl>}
                      {task.projectId != null && props.projectManagement && props.onOpenTaskProject && <Button variant="outline" disabled={props.busy} onClick={() => props.onOpenTaskProject?.(task.projectId!)}>查看所属项目 ##{task.projectId}</Button>}
                      {detailTab === 'overview' && <ProjectProfilePanel profile={props.taskContext?.projectProfile} />}
                      {props.taskContext && <TaskDetailPanel tab={detailTab} context={props.taskContext} notes={props.notes} sessions={props.sessions} history={props.history} />}
                    </div>
                  ) : <EmptyState className="px-6 py-16" title={props.selectedTaskId === null ? '选择任务查看详情' : props.busy ? '正在加载任务详情' : '任务详情暂不可用'} description="从左侧任务列表选择一项。" /> : detail ? (
                    <div className="space-y-6">
                      <h3 className="break-words text-xl font-semibold">{detail.project.name}</h3>
                      <dl className="m-0 space-y-4">
                        <Field label="项目 ID" value={detail.project.id} />
                        <Field label="修订" value={detail.project.revision} />
                        <Field label="创建时间" value={detail.project.createdAt} />
                        <Field label="更新时间" value={detail.project.updatedAt} />
                      </dl>
                      <ProjectProfilePanel profile={detail.profile} />
                      {props.onViewProjectTasks && <Button variant="outline" disabled={props.busy} onClick={() => props.onViewProjectTasks?.(detail.project.id)}>查看关联任务</Button>}
                      <section aria-label="项目历史">
                        <h3 className="mb-3 font-semibold">项目历史</h3>
                        <HistoryPanel entries={props.projectHistory} />
                        {props.projectHistoryHasMore && <Button variant="outline" disabled={props.busy || !props.onMoreProjectHistory} onClick={props.onMoreProjectHistory}>加载更多项目历史</Button>}
                      </section>
                      <section aria-label="项目组件">
                        <h3 className="mb-3 font-semibold">组件</h3>
                        {detail.components.length ? <ul className="space-y-2 pl-5">{detail.components.map((item) => <li key={item.id} className="break-words text-sm">#{item.id} · {item.name}</li>)}</ul> : <EmptyState className="px-4 py-8" title="暂无组件" />}
                      </section>
                      <section aria-label="项目源码">
                        <h3 className="mb-3 font-semibold">源码</h3>
                        {detail.sources.length ? <ul className="list-none space-y-3 p-0">{detail.sources.map((source) => (
                          <li key={source.id} className="rounded-lg border border-border p-4">
                            <dl className="m-0 space-y-2">
                              <Field label="源码 ID" value={source.id} />
                              <Field label="组件 ID" value={source.componentId} />
                              <Field label="仓库 ID" value={source.repositoryId} />
                              <Field label="相对路径" value={source.relativePath} />
                              <Field label="目录路径" value={source.directoryPath} />
                            </dl>
                          </li>
                        ))}</ul> : <EmptyState className="px-4 py-8" title="暂无源码" />}
                      </section>
                    </div>
                  ) : <EmptyState className="px-6 py-16" title={props.selectedProjectId === null ? '选择项目查看详情' : props.busy ? '正在加载项目详情' : '项目详情暂不可用'} description="从左侧项目列表选择一项。" />}
                </CardContent>
              </Card>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
