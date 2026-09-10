import { create } from 'zustand';
import type { DetailTab } from '../lib/contracts';

type Tab = 'tasks' | 'projects';
interface WorkspaceState {
  tab: Tab;
  view: string;
  searchInput: string;
  query: string;
  projectSearchInput: string;
  projectQuery: string;
  projectFilter: number | null;
  detailTab: DetailTab;
  selectedTaskId: number | null;
  selectedProjectId: number | null;
  setTab: (tab: Tab) => void;
  setView: (view: string) => void;
  setSearchInput: (input: string) => void;
  applySearch: () => void;
  setProjectSearchInput: (input: string) => void;
  applyProjectSearch: () => void;
  setDetailTab: (tab: DetailTab) => void;
  viewProjectTasks: (id: number) => void;
  openTaskProject: (id: number) => void;
  clearProjectFilter: () => void;
  disableProjects: () => void;
  selectTask: (id: number) => void;
  selectProject: (id: number) => void;
  reset: () => void;
}
const initial = { tab: 'tasks' as Tab, view: 'active', searchInput: '', query: '', projectSearchInput: '', projectQuery: '', projectFilter: null, detailTab: 'overview' as DetailTab, selectedTaskId: null, selectedProjectId: null };
// Only ephemeral UI state. Tasks/projects live exclusively in the Query cache.
export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  ...initial,
  setTab: (tab) => set({ tab }),
  setView: (view) => set({ view, selectedTaskId: null }),
  setSearchInput: (searchInput) => set({ searchInput }),
  applySearch: () => set((state) => ({ query: state.searchInput.trim(), selectedTaskId: null })),
  setProjectSearchInput: (projectSearchInput) => set({ projectSearchInput }),
  applyProjectSearch: () => set((state) => ({ projectQuery: state.projectSearchInput.trim(), selectedProjectId: null })),
  setDetailTab: (detailTab) => set({ detailTab }),
  viewProjectTasks: (projectFilter) => set({ tab: 'tasks', projectFilter, selectedTaskId: null, detailTab: 'overview' }),
  openTaskProject: (selectedProjectId) => set({ tab: 'projects', selectedProjectId, projectSearchInput: '', projectQuery: '' }),
  clearProjectFilter: () => set({ projectFilter: null, selectedTaskId: null, detailTab: 'overview' }),
  disableProjects: () => set({ tab: 'tasks', projectFilter: null, projectQuery: '', projectSearchInput: '', selectedProjectId: null, selectedTaskId: null, detailTab: 'overview' }),
  selectTask: (selectedTaskId) => set({ selectedTaskId, detailTab: 'overview' }),
  selectProject: (selectedProjectId) => set({ selectedProjectId }),
  reset: () => set(initial),
}));
