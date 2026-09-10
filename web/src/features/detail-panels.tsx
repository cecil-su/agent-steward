import type { DetailTab, HistoryEntry, Note, Session, TaskContext, ProjectProfile } from '../lib/contracts';
import { EmptyState } from '../components/ui/empty-state';

export function ProjectProfilePanel({ profile }: { profile?: ProjectProfile | null }) {
  return (
    <section aria-label="项目资料" className="space-y-3 rounded-lg border border-border bg-card p-4">
      <h3 className="font-semibold">项目资料</h3>
      {profile === undefined ? (
        <EmptyState className="px-4 py-8" title="项目资料不可用" description="后端未提供资料字段，旧后端可能不支持；不能据此认定资料尚未维护。" />
      ) : profile === null ? (
        <EmptyState className="px-4 py-8" title="项目资料尚未维护" description="当前项目尚无已记录资料。" />
      ) : (
        <>
          <p className="text-sm text-muted-foreground">以下为已记录资料及其来源依据，不代表已自动验证，也不代表项目或源码的实时状态；来源任务版本是记录时版本。</p>
          <dl className="m-0 space-y-4 text-sm">
            {([
              ['项目编号', `##${profile.projectId}`],
              ['资料简介', profile.summary],
              ['架构入口', profile.architecture],
              ['开发验证', profile.development],
              ['依据', profile.evidence],
              ['来源 Task', `#${profile.sourceTaskId}`],
              ['来源 Task 记录版本', profile.sourceTaskVersion],
              ['资料修订', profile.revision],
              ['资料更新时间', profile.updatedAt],
            ] as const).map(([label, value]) => (
              <div key={label} className="space-y-1">
                <dt className="text-xs text-muted-foreground">{label}</dt>
                <dd className="m-0 whitespace-pre-wrap break-words">{value}</dd>
              </div>
            ))}
          </dl>
        </>
      )}
    </section>
  );
}

export function HistoryPanel({ entries }: { entries?: HistoryEntry[] }) {
  if (!entries?.length) return <EmptyState className="px-4 py-8" title={entries ? '暂无历史' : '历史尚未提供'} />;
  return <ol className="list-none space-y-3 p-0">{entries.map((entry, index) => (
    <li key={`${entry.sequence ?? entry.revision ?? index}-${index}`} className="rounded-lg border border-border p-4 text-sm">
      <p className="text-muted-foreground">{entry.occurredAt} · {entry.changeType} · {entry.sequence != null ? `序号 ${entry.sequence}` : `修订 ${entry.revision ?? '未知'}`}</p>
      {entry.summary != null && <p className="whitespace-pre-wrap break-words">{entry.summary}</p>}
      {entry.payload !== undefined && <details><summary className="cursor-pointer text-primary">原始记录</summary><Json value={entry.payload} /></details>}
    </li>
  ))}</ol>;
}

function Json({ value }: { value: unknown }) {
  return <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-muted p-4 text-xs">{JSON.stringify(value, null, 2)}</pre>;
}

function NotesPanel({ notes }: { notes?: Note[] }) {
  if (!notes?.length) return <EmptyState className="px-4 py-8" title={notes ? '暂无进展备注' : '进展备注尚未提供'} />;
  return <ul className="list-none space-y-3 p-0">{notes.map((note) => (
    <li key={note.id} className="rounded-lg border border-border p-4 text-sm">
      <p className="text-xs text-muted-foreground">#{note.id} · {note.noteType} · {note.createdAt}</p>
      <p className="whitespace-pre-wrap break-words">{note.text}</p>
    </li>
  ))}</ul>;
}

function SessionPanel({ sessions }: { sessions?: Session[] }) {
  if (!sessions?.length) return <EmptyState className="px-4 py-8" title={sessions ? '暂无 Session' : 'Session 尚未提供'} />;
  return <ul className="list-none space-y-3 p-0">{sessions.map((session) => (
    <li key={session.id} className="rounded-lg border border-border p-4 text-sm">
      <dl className="space-y-2 break-words">
        {Object.entries({ 'Session ID': session.id, '来源': session.source, '外部 Session ID': session.externalSessionId, '续接自': session.continuedFrom, '记录路径': session.recordPath, '开始时间': session.startedAt, '结束时间': session.endedAt }).map(([label, value]) => (
          <div key={label}><dt className="text-xs text-muted-foreground">{label}</dt><dd className="m-0 whitespace-pre-wrap">{value ?? '未提供'}</dd></div>
        ))}
      </dl>
    </li>
  ))}</ul>;
}

export function TaskDetailPanel({ tab, context, notes, sessions, history }: {
  tab: DetailTab; context: TaskContext; notes?: Note[]; sessions?: Session[]; history?: HistoryEntry[];
}) {
  if (tab === 'notes') return <NotesPanel notes={notes} />;
  if (tab === 'sessions') return <SessionPanel sessions={sessions} />;
  if (tab === 'history') return <HistoryPanel entries={history} />;
  if (tab === 'worktree') return (
    <section aria-label="代码现场" className="space-y-3">
      <p className="text-sm text-muted-foreground">仅展示后端提供的代码现场观测 JSON，不执行 Git 写操作。</p>
      {context.worktreeStatus == null
        ? <EmptyState className="px-4 py-8" title="代码现场不可观察" description="未提供 worktreeStatus，不能据此认定工作树为 clean。" />
        : <Json value={context.worktreeStatus} />}
    </section>
  );
  const checkpoint = context.checkpoint;
  return <div className="space-y-6">
    <section aria-label="Checkpoint">
      <h3 className="font-semibold">Checkpoint</h3>
      {checkpoint ? <div className="space-y-3 text-sm">
        <p className="text-xs text-muted-foreground">{checkpoint.createdAt} · {checkpoint.sessionId}</p>
        <p className="whitespace-pre-wrap break-words">{checkpoint.summary}</p>
        {([['已完成', checkpoint.completed], ['决策', checkpoint.decisions], ['待办', checkpoint.pending], ['风险', checkpoint.risks]] as const).map(([label, items]) => (
          <div key={label}><h4 className="text-muted-foreground">{label}</h4>{items.length ? <ul className="list-disc pl-5">{items.map((item, index) => <li key={index} className="whitespace-pre-wrap break-words">{item}</li>)}</ul> : <p>无</p>}</div>
        ))}
        <p className="whitespace-pre-wrap break-words">下一步：{checkpoint.nextStep ?? '未提供'}</p>
      </div> : <EmptyState className="px-4 py-8" title="暂无 Checkpoint" />}
    </section>
    <section aria-label="近期备注">
      <h3 className="font-semibold">Checkpoint 后的近期备注</h3>
      {context.notesTruncated && <p role="status" className="my-3 rounded-lg border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900">近期备注已截断，并非完整记录；请查看进展备注。</p>}
      <NotesPanel notes={context.notesSinceCheckpoint} />
    </section>
    {context.session && <section aria-label="当前 Session"><h3 className="font-semibold">当前 Session</h3><SessionPanel sessions={[context.session]} /></section>}
  </div>;
}
