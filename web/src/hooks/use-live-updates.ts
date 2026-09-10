import { useEffect, useRef, useState } from 'react';
import { subscribeLiveEvents } from '../lib/live-events';

interface Options { protectedInput: boolean; refresh: () => Promise<unknown>; unauthorized: () => void; revalidate: () => Promise<void> }
export function useLiveUpdates(enabled: boolean, options: Options) {
  const latest = useRef(options);
  latest.current = options;
  const resume = useRef<(() => void) | null>(null);
  const [status, setStatus] = useState('未连接');
  const [pending, setPending] = useState(false);
  useEffect(() => {
    setPending(false);
    if (!enabled) { setStatus('未连接'); return; }
    let active = true;
    let refreshing = false;
    let dirty = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const pump = () => {
      if (!active || !dirty || refreshing || timer || latest.current.protectedInput) return;
      timer = setTimeout(async () => {
        timer = undefined;
        if (!active || latest.current.protectedInput) return;
        dirty = false; setPending(false); refreshing = true;
        try { await latest.current.refresh(); }
        catch { if (active) setStatus('刷新失败，请手动重试'); }
        finally { refreshing = false; if (active) pump(); }
      }, 250);
    };
    resume.current = pump;
    const stop = subscribeLiveEvents({
      status: (value) => { if (active) setStatus(value); },
      changed: () => { if (active) { dirty = true; setPending(true); pump(); } },
      unauthorized: () => { if (active) latest.current.unauthorized(); },
      revalidate: () => active ? latest.current.revalidate() : Promise.resolve(),
    });
    return () => { active = false; clearTimeout(timer); resume.current = null; stop(); };
  }, [enabled]);
  useEffect(() => { if (!options.protectedInput) resume.current?.(); }, [options.protectedInput]);
  return pending && options.protectedInput ? '有更新，当前输入已保留；应用搜索或完成复制后刷新' : status;
}
