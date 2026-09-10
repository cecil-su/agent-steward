// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Project, ProjectProfile, Task } from '../lib/contracts';
import { ReadonlyWorkspace, type ReadonlyWorkspaceProps } from './workspace';

const task: Task = {
  id: 12, title: '展示任务', status: 'in_progress', version: 3,
  goal: '示例目标', scope: '示例范围', acceptanceCriteria: '示例验收标准',
  nextStep: '示例下一步', projectId: 7, componentIds: [4],
};
const project: Project = { id: 7, name: '展示项目', revision: 2, createdAt: '2026-01-01', updatedAt: '2026-01-02' };

function makeProps(overrides: Partial<ReadonlyWorkspaceProps> = {}): ReadonlyWorkspaceProps {
  return {
    tab: 'tasks', onTabChange: vi.fn(), tasks: [task], projects: [project],
    selectedTaskId: null, selectedProjectId: null, onSelectTask: vi.fn(), onSelectProject: vi.fn(),
    projectManagement: true, busy: false, error: null, hasMore: false, onMore: vi.fn(),
    onRefresh: vi.fn(), query: '', onQueryChange: vi.fn(), onSearch: vi.fn(),
    view: 'active', onViewChange: vi.fn(), ...overrides,
  };
}

const profile: ProjectProfile = {
  projectId: 7, revision: 9, summary: '资料概要\n第二行', architecture: '架构入口说明',
  development: '开发验证记录', evidence: '公开来源依据', sourceTaskId: 40,
  sourceTaskVersion: 6, updatedAt: '2026-09-09T10:00:00Z',
};

afterEach(cleanup);

describe('ReadonlyWorkspace', () => {
  it.each(['tasks', 'projects'] as const)('displays recorded project profile in %s without write controls', (tab) => {
    const { rerender } = render(<ReadonlyWorkspace {...makeProps({
      tab, selectedTaskId: task.id, taskContext: { task, project, projectProfile: profile },
      selectedProjectId: project.id, projectDetail: { project, components: [], sources: [], profile },
    })} />);
    const panel = within(screen.getByRole('region', { name: '项目资料' }));
    for (const [label, value] of [
      ['项目编号', '##7'], ['资料简介', '资料概要\n第二行'], ['架构入口', '架构入口说明'],
      ['开发验证', '开发验证记录'], ['依据', '公开来源依据'], ['来源 Task', '#40'],
      ['来源 Task 记录版本', '6'], ['资料修订', '9'], ['资料更新时间', '2026-09-09T10:00:00Z'],
    ]) {
      expect(panel.getByText(label, { selector: 'dt' }).nextElementSibling?.textContent).toBe(value);
    }
    expect(panel.getByText(/不代表已自动验证/)).toBeTruthy();
    expect(panel.getByText(/不代表项目或源码的实时状态/)).toBeTruthy();
    expect(panel.queryByRole('button')).toBeNull();
    expect(screen.queryByRole('button', { name: /新建|保存|编辑|删除|维护资料/ })).toBeNull();
    if (tab === 'tasks') {
      rerender(<ReadonlyWorkspace {...makeProps({ selectedTaskId: task.id, taskContext: { task, projectProfile: profile }, detailTab: 'notes' })} />);
      expect(screen.queryByRole('region', { name: '项目资料' })).toBeNull();
    }
  });

  it.each(['tasks', 'projects'] as const)('distinguishes unsupported profile from unmaintained profile in %s', (tab) => {
    const props = makeProps({ tab, selectedTaskId: task.id, taskContext: { task },
      selectedProjectId: project.id, projectDetail: { project, components: [], sources: [] },
    });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByText('项目资料不可用')).toBeTruthy();
    expect(screen.getByText(/旧后端可能不支持/)).toBeTruthy();
    expect(screen.queryByText('项目资料尚未维护')).toBeNull();
    rerender(<ReadonlyWorkspace {...props} taskContext={{ task, projectProfile: null }} projectDetail={{ project, components: [], sources: [], profile: null }} />);
    expect(screen.getByText('项目资料尚未维护')).toBeTruthy();
    expect(screen.queryByText('项目资料不可用')).toBeNull();
    rerender(<ReadonlyWorkspace {...props} selectedTaskId={99} selectedProjectId={99} />);
    expect(screen.queryByRole('region', { name: '项目资料' })).toBeNull();
  });

  it('uses theme tokens without duplicating the parent page spacing', () => {
    const { container } = render(<ReadonlyWorkspace {...makeProps()} />);
    const root = container.firstElementChild as HTMLElement;
    expect(root.classList.contains('bg-background')).toBe(true);
    expect(root.classList.contains('text-foreground')).toBe(true);
    for (const wrapper of [root, root.firstElementChild as HTMLElement]) {
      expect(wrapper.className).not.toMatch(/(?:^|\s)(?:\S*:)?(?:p[xy]?-[\w-]+|max-w-\S+|min-h-screen)/);
    }
    const listItem = screen.getByRole('button', { name: /展示任务/ });
    expect(listItem.textContent).toContain('#12');
    expect(listItem.textContent).not.toContain('##12');
    for (const token of ['border-border', 'bg-card', 'focus-visible:ring-ring', 'aria-pressed:border-primary']) {
      expect(listItem.classList.contains(token)).toBe(true);
    }
  });

  it.each(['tasks', 'projects'] as const)('keeps %s styling unified with no business write controls', (tab) => {
    const { container } = render(<ReadonlyWorkspace {...makeProps({
      tab, selectedTaskId: task.id, taskContext: { task, project },
      selectedProjectId: project.id, projectDetail: { project, components: [], sources: [] },
    })} />);
    expect(container.querySelector('.bg-card')).toBeTruthy();
    expect(container.querySelector('.text-muted-foreground')).toBeTruthy();
    expect(container.querySelector('.border-input')).toBeTruthy();
    const activeTab = within(screen.getByRole('navigation', { name: '工作区' })).getByRole('button', { name: tab === 'tasks' ? '任务' : '项目' });
    expect(activeTab.classList.contains('bg-primary')).toBe(true);
    expect(activeTab.classList.contains('text-primary-foreground')).toBe(true);
    const refresh = screen.getByRole('button', { name: '刷新' });
    const search = screen.getByRole('button', { name: '搜索' });
    for (const token of ['h-10', 'border-border', 'bg-card']) {
      expect(refresh.classList.contains(token)).toBe(search.classList.contains(token));
    }
    expect(refresh.classList.contains('min-w-24')).toBe(true);
    expect(refresh.classList.contains('h-10')).toBe(true);
    expect(refresh.classList.contains('border-border')).toBe(true);
    expect(screen.queryByRole('button', { name: /新建|创建|保存|编辑|删除|改名|改标题|提交|领取|恢复|关闭任务/ })).toBeNull();
    expect(container.innerHTML).not.toMatch(/#(?:236b50|243831|e2e7df|768077|ccd6cb|72b69c|33493e|eef3ed|f5f6f2)/i);
  });

  it('marks the workspace read-only and exposes no pretend write actions', () => {
    render(<ReadonlyWorkspace {...makeProps()} />);
    expect(screen.queryByText('只读工作区')).toBeNull();
    expect(screen.queryByText(/不提供业务写入口/)).toBeNull();
    expect(screen.queryByRole('button', { name: /新建|保存|编辑|删除/ })).toBeNull();
    expect(screen.getByText('选择任务查看详情')).toBeTruthy();
  });

  it('delegates search, selection, views, refresh, pagination and navigation', () => {
    const props = makeProps({ hasMore: true, query: '当前查询' });
    render(<ReadonlyWorkspace {...props} />);
    const input = screen.getByRole('searchbox', { name: '搜索任务' });
    expect((input as HTMLInputElement).value).toBe('当前查询');
    fireEvent.change(input, { target: { value: '新查询' } });
    expect(props.onQueryChange).toHaveBeenCalledWith('新查询');
    fireEvent.submit(screen.getByRole('search'));
    expect(props.onSearch).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: /展示任务/ }));
    expect(props.onSelectTask).toHaveBeenCalledWith(12);
    const views = within(screen.getByLabelText('任务状态视图'));
    for (const [label, value] of [['未关闭', 'active'], ['进行中', 'in-progress'], ['有阻塞', 'blocked'], ['已关闭', 'closed'], ['最近全部', 'recent']]) {
      fireEvent.click(views.getByRole('button', { name: label }));
      expect(props.onViewChange).toHaveBeenLastCalledWith(value);
    }
    expect(views.getByRole('button', { name: '未关闭' }).getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: '刷新' }));
    fireEvent.click(screen.getByRole('button', { name: '加载更多' }));
    fireEvent.click(screen.getByRole('button', { name: '项目' }));
    expect(props.onRefresh).toHaveBeenCalledOnce();
    expect(props.onMore).toHaveBeenCalledOnce();
    expect(props.onTabChange).toHaveBeenCalledWith('projects');
  });

  it('renders task fields and ignores stale detail', () => {
    const props = makeProps({ selectedTaskId: 12, taskContext: { task, project } });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    for (const value of ['示例目标', '示例范围', '示例验收标准', '示例下一步', '展示项目']) {
      expect(screen.getByText(value)).toBeTruthy();
    }
    expect(screen.getByRole('button', { name: /展示任务/ }).getAttribute('aria-pressed')).toBe('true');
    rerender(<ReadonlyWorkspace {...props} selectedTaskId={99} />);
    expect(screen.queryByText('示例目标')).toBeNull();
    expect(screen.getByText('任务详情暂不可用')).toBeTruthy();
  });

  it('renders legacy task detail without project or component fields', () => {
    const legacyTask: Task = {
      id: 13, title: '旧后端任务', status: 'open', version: 1,
      goal: '旧后端目标', scope: null, acceptanceCriteria: null, nextStep: null,
    };
    render(<ReadonlyWorkspace {...makeProps({
      tasks: [legacyTask], selectedTaskId: legacyTask.id,
      taskContext: { task: legacyTask }, projectManagement: false,
    })} />);
    expect(screen.getByRole('heading', { name: '旧后端任务' })).toBeTruthy();
    expect(screen.getByText('旧后端目标')).toBeTruthy();
    expect(screen.getByText('项目', { selector: 'dt' }).nextElementSibling?.textContent).toBe('未设置');
    expect(screen.getByText('组件 ID', { selector: 'dt' }).nextElementSibling?.textContent).toBe('无');
  });

  it('renders project components and source paths with exact project lookup', () => {
    const props = makeProps({
      tab: 'projects', selectedProjectId: 7, hasMore: true,
      projectDetail: {
        project, components: [{ id: 4, name: '服务组件' }],
        sources: [{ id: 9, componentId: 4, repositoryId: 5, relativePath: 'src/example.ts', directoryPath: '/example/source' }],
      },
    });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByRole('searchbox', { name: '查找项目（唯一名称或 ##编号）' })).toBeTruthy();
    expect(screen.getByText(/不是模糊全库搜索/)).toBeTruthy();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: '##7' } });
    fireEvent.submit(screen.getByRole('search'));
    expect(props.onQueryChange).toHaveBeenCalledWith('##7');
    expect(props.onSearch).toHaveBeenCalledOnce();
    expect(screen.queryByLabelText('任务状态视图')).toBeNull();
    expect(within(screen.getByRole('region', { name: '项目组件' })).getByText(/服务组件/)).toBeTruthy();
    expect(screen.getByRole('button', { name: /展示项目/ }).textContent).toContain('##7 · 修订 2');
    expect(screen.getByText('src/example.ts')).toBeTruthy();
    expect(screen.getByText('/example/source')).toBeTruthy();
    expect(screen.queryByRole('link')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /展示项目/ }));
    fireEvent.click(screen.getByRole('button', { name: '返回任务' }));
    fireEvent.click(screen.getByRole('button', { name: '加载更多' }));
    expect(props.onSelectProject).toHaveBeenCalledWith(7);
    expect(props.onTabChange).toHaveBeenCalledWith('tasks');
    expect(props.onMore).toHaveBeenCalledOnce();
    rerender(<ReadonlyWorkspace {...props} selectedProjectId={99} />);
    expect(screen.queryByText('src/example.ts')).toBeNull();
    expect(screen.getByText('项目详情暂不可用')).toBeTruthy();
  });

  it('shows errors, loading and disables interactions while busy', () => {
    const props = makeProps({ busy: true, error: '读取失败', hasMore: true });
    render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByRole('alert').textContent).toBe('读取失败');
    expect(screen.getByRole('status').textContent).toBe('正在加载…');
    for (const button of screen.getAllByRole('button')) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
      fireEvent.click(button);
    }
    expect((screen.getByRole('searchbox') as HTMLInputElement).disabled).toBe(true);
    fireEvent.submit(screen.getByRole('search'));
    for (const callback of [props.onSearch, props.onMore, props.onRefresh, props.onSelectTask, props.onTabChange, props.onViewChange]) {
      expect(callback).not.toHaveBeenCalled();
    }
  });

  it('renders padded empty states and hides unavailable pagination', () => {
    const props = makeProps({ tasks: [], projects: [] });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByText('暂无任务').parentElement?.className).toContain('px-4');
    expect(screen.getByText('选择任务查看详情').parentElement?.className).toContain('px-6');
    expect(screen.queryByRole('button', { name: '加载更多' })).toBeNull();
    rerender(<ReadonlyWorkspace {...props} tab="projects" />);
    expect(screen.getByText('暂无项目')).toBeTruthy();
    expect(screen.getByText('选择项目查看详情')).toBeTruthy();
    rerender(<ReadonlyWorkspace {...props} tab="projects" selectedProjectId={7} projectDetail={{ project, components: [], sources: [] }} />);
    expect(screen.getByText('暂无组件')).toBeTruthy();
    expect(screen.getByText('暂无源码')).toBeTruthy();
  });

  it('renders checkpoint and recent notes with truncation warning and controlled detail navigation', () => {
    const props = makeProps({ selectedTaskId: 12, onDetailTabChange: vi.fn(), taskContext: {
      task, checkpoint: { summary: '检查点摘要', completed: ['完成内容'], decisions: ['决策内容'], pending: ['待办内容'], risks: ['风险内容'], nextStep: '后续工作', createdAt: '2026-01-03', sessionId: 'session-1' },
      notesSinceCheckpoint: [{ id: 1, noteType: 'progress', text: '近期进展', createdAt: '2026-01-04' }], notesTruncated: true,
    } });
    render(<ReadonlyWorkspace {...props} />);
    for (const text of ['检查点摘要', '完成内容', '决策内容', '待办内容', '风险内容', '近期进展']) expect(screen.getByText(text)).toBeTruthy();
    expect(screen.getByRole('status').textContent).toContain('已截断');
    const nav = within(screen.getByLabelText('任务详情导航'));
    for (const [label, value] of [['概览', 'overview'], ['进展备注', 'notes'], ['Session', 'sessions'], ['代码现场', 'worktree'], ['历史', 'history']]) {
      fireEvent.click(nav.getByRole('button', { name: label }));
      expect(props.onDetailTabChange).toHaveBeenLastCalledWith(value);
    }
    expect(screen.getByText('检查点摘要')).toBeTruthy();
  });

  it('renders notes, sessions, worktree observations and history from props', () => {
    const props = makeProps({ selectedTaskId: 12, taskContext: { task },
      notes: [{ id: 2, noteType: 'decision', text: '完整备注', createdAt: '2026-01-04' }],
      sessions: [{ id: 'session-2', source: 'pi', externalSessionId: 'external', continuedFrom: 'session-1', recordPath: '/records/example', startedAt: '2026-01-01' }],
      history: [{ sequence: 8, changeType: 'note', occurredAt: '2026-01-04', summary: '任务历史内容', payload: { example: true } }],
    });
    const { rerender } = render(<ReadonlyWorkspace {...props} detailTab="notes" />);
    expect(screen.getByText('完整备注')).toBeTruthy();
    expect(screen.queryByText('示例目标')).toBeNull();
    rerender(<ReadonlyWorkspace {...props} detailTab="sessions" />);
    expect(screen.getByText('session-2')).toBeTruthy();
    expect(screen.getByText('/records/example')).toBeTruthy();
    rerender(<ReadonlyWorkspace {...props} detailTab="worktree" />);
    expect(screen.getByText('代码现场不可观察')).toBeTruthy();
    expect(screen.getByText(/不能据此认定工作树为 clean/)).toBeTruthy();
    rerender(<ReadonlyWorkspace {...props} detailTab="worktree" taskContext={{ task, worktreeStatus: { dirty: true, branch: 'example' } }} />);
    expect(screen.getByText(/"dirty": true/)).toBeTruthy();
    rerender(<ReadonlyWorkspace {...props} detailTab="history" />);
    expect(screen.getByText('任务历史内容')).toBeTruthy();
    expect(screen.getByText(/"example": true/)).toBeTruthy();
  });

  it('delegates context copying, project navigation and clearing the project filter', () => {
    const props = makeProps({ selectedTaskId: 12, taskContext: { task, project }, projectFilter: 7,
      onCopyContext: vi.fn(), copyText: '只读上下文', onClearProjectFilter: vi.fn(), onOpenTaskProject: vi.fn(),
    });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByText('当前项目筛选：##7')).toBeTruthy();
    const textarea = screen.getByRole('textbox', { name: '上下文（可手工复制）' }) as HTMLTextAreaElement;
    expect(textarea.readOnly).toBe(true);
    expect(textarea.value).toBe('只读上下文');
    fireEvent.click(screen.getByRole('button', { name: '复制上下文' }));
    fireEvent.click(screen.getByRole('button', { name: '清除项目筛选' }));
    fireEvent.click(screen.getByRole('button', { name: '查看所属项目 ##7' }));
    expect(props.onCopyContext).toHaveBeenCalledOnce();
    expect(props.onClearProjectFilter).toHaveBeenCalledOnce();
    expect(props.onOpenTaskProject).toHaveBeenCalledWith(7);
    rerender(<ReadonlyWorkspace {...props} busy />);
    expect((screen.getByRole('button', { name: '复制上下文' }) as HTMLButtonElement).disabled).toBe(true);
    rerender(<ReadonlyWorkspace {...props} selectedTaskId={99} />);
    expect(screen.queryByRole('button', { name: '复制上下文' })).toBeNull();
    expect(screen.queryByDisplayValue('只读上下文')).toBeNull();
  });

  it('delegates project task navigation and history pagination', () => {
    const props = makeProps({ tab: 'projects', selectedProjectId: 7, projectDetail: { project, components: [], sources: [] },
      onViewProjectTasks: vi.fn(), projectHistory: [{ revision: 3, changeType: 'update', occurredAt: '2026-01-04', summary: '项目历史内容' }],
      projectHistoryHasMore: true, onMoreProjectHistory: vi.fn(),
    });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.getByText('项目历史内容')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '查看关联任务' }));
    fireEvent.click(screen.getByRole('button', { name: '加载更多项目历史' }));
    expect(props.onViewProjectTasks).toHaveBeenCalledWith(7);
    expect(props.onMoreProjectHistory).toHaveBeenCalledOnce();
    rerender(<ReadonlyWorkspace {...props} busy />);
    expect((screen.getByRole('button', { name: '加载更多项目历史' }) as HTMLButtonElement).disabled).toBe(true);
    rerender(<ReadonlyWorkspace {...props} projectHistoryHasMore={false} />);
    expect(screen.queryByRole('button', { name: '加载更多项目历史' })).toBeNull();
  });

  it('gates project browsing when project management is unavailable', () => {
    const props = makeProps({ projectManagement: false });
    const { rerender } = render(<ReadonlyWorkspace {...props} />);
    expect(screen.queryByRole('button', { name: '项目' })).toBeNull();
    rerender(<ReadonlyWorkspace {...props} tab="projects" />);
    expect(screen.getByText('项目浏览不可用')).toBeTruthy();
    expect(screen.queryByText('项目列表')).toBeNull();
    expect(screen.queryByRole('search')).toBeNull();
  });
});
