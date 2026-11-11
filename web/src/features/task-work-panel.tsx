import type { TaskWork } from '../lib/task-work';
import { requirementLabels, validWork } from '../lib/task-work';
import { Markdown } from '../components/markdown';

export function TaskWorkPanel({ work }: { work?: TaskWork | null }) {
  const box = 'min-w-0 space-y-3 rounded-lg border border-border bg-card p-4 [overflow-wrap:anywhere]';
  if (work === undefined) return <section aria-label="需求进展" className={box}><h3 className="font-semibold">需求进展</h3><p>需求管理尚未接入，不能据此判断没有需求。</p></section>;
  if (work === null) return <section aria-label="需求进展" className={box}><h3 className="font-semibold">需求进展</h3><p>尚未整理为需求项；不代表任务没有需求。由 Agent 按实际需求整理。</p></section>;
  if (!validWork(work)) return <section aria-label="需求进展" className={box}><h3 className="font-semibold">需求进展</h3><p role="status">需求数据格式不可用，请重新核对；不以空清单代替。</p></section>;
  const open = work.confirmations.filter(c => c.status === 'open');
  const closed = work.confirmations.filter(c => c.status !== 'open');
  return <section aria-label="需求进展" className="space-y-4">
    <h3 className="font-semibold">需求进展</h3>
    <p className="text-sm text-muted-foreground">{work.requirements.length} 项需求 · {work.requirements.filter(r => r.status === 'done').length} 项已完成 · {open.length} 项待你确认</p>
    <section aria-label="待你确认" className={box}>
      <h4 className="font-semibold">待你确认</h4>
      {!open.length && <p>暂无需要你确认的事项。</p>}
      {open.map(c => <article key={c.id} className="space-y-2 border-t border-border pt-3 text-sm">
        <h5 className="font-semibold">{c.question}</h5><p className="text-muted-foreground">关联需求：{c.requirementIds.join('、')}</p>
        <div>建议：<Markdown text={c.recommendation} /></div><div>影响：<Markdown text={c.impact} /></div>
      </article>)}
    </section>
    <ol className="list-none space-y-3 p-0">{work.requirements.map(r => <li key={r.id} className={box}>
      <div className="flex items-start justify-between gap-3"><h4 className="min-w-0 font-semibold">{r.id} · {r.title}</h4><span className="shrink-0 rounded border border-border px-2 py-1 text-xs">{requirementLabels[r.status]}</span></div>
      <Markdown text={r.result || '尚未形成结果。'} />
      {r.nextStep && <div className="text-sm">下一步：<Markdown text={r.nextStep} /></div>}
      <details className="text-sm"><summary className="min-h-8 cursor-pointer text-primary">完成条件与依据</summary><div className="space-y-2 pt-2"><Markdown text={r.acceptanceCriteria} />{r.evidence.map((e, i) => <Markdown key={i} text={e} />)}<p className="text-xs text-muted-foreground">记录更新时间：{r.updatedAt}</p></div></details>
    </li>)}</ol>
    {!!closed.length && <details className={box}><summary className="cursor-pointer text-primary">已处理的确认事项（{closed.length}）</summary>{closed.map(c => <article key={c.id} className="space-y-2 border-t border-border pt-3 text-sm"><h4 className="font-semibold">{c.question}</h4><p>{c.status === 'resolved' ? '已答复' : '已撤回'}</p><Markdown text={c.answer} />{c.evidence.map((e, i) => <Markdown key={i} text={e} />)}</article>)}</details>}
  </section>;
}
