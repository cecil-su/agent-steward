export type TaskStatus = 'open' | 'in_progress' | 'blocked' | 'closed';
export interface Task {
  id: number;
  title: string | null;
  status: TaskStatus;
  version: number;
  goal: string | null;
  scope: string | null;
  acceptanceCriteria: string | null;
  nextStep: string | null;
  projectId?: number | null;
  componentIds?: number[];
}
export interface Project {
  id: number;
  name: string;
  revision: number;
  createdAt: string;
  updatedAt: string;
}
export interface ProjectProfile {
  projectId: number; revision: number; summary: string; architecture: string;
  development: string; evidence: string; sourceTaskId: number; sourceTaskVersion: number; updatedAt: string;
}
export interface Access { role: 'admin' | 'reader'; local: boolean; projectManagement?: boolean }
export interface Warning { code: string; message: string }
export interface TaskPage { tasks: Task[]; hasMore: boolean; nextCursor: string | null }
export interface ProjectPage { projects: Project[]; hasMore: boolean; nextAfter: number | null }
export interface Checkpoint { summary: string; completed: string[]; decisions: string[]; pending: string[]; risks: string[]; nextStep?: string | null; createdAt: string; sessionId: string }
export interface Note { id: number; noteType: string; text: string; createdAt: string }
export interface Session { id: string; source?: string | null; externalSessionId?: string | null; continuedFrom?: string | null; recordPath?: string | null; startedAt: string; endedAt?: string | null }
export interface HistoryEntry { sequence?: number; revision?: number; changeType: string; occurredAt: string; summary?: string; payload?: unknown }
export interface ProjectHistoryPage { history: HistoryEntry[]; hasMore: boolean; nextAfter: number | null }
export type DetailTab = 'overview' | 'notes' | 'sessions' | 'worktree' | 'history';
export interface TaskContext { task: Task; project?: Project | null; projectProfile?: ProjectProfile | null; checkpoint?: Checkpoint | null; notesSinceCheckpoint?: Note[]; notesTruncated?: boolean; session?: Session | null; worktreeStatus?: unknown }
export interface Component { id: number; name: string }
export interface Source { id: number; componentId: number | null; repositoryId: number | null; relativePath: string | null; directoryPath: string | null }
export interface ProjectDetail { project: Project; profile?: ProjectProfile | null; components: Component[]; sources: Source[] }
