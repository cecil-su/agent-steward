import { useEffect, useRef, useState } from 'react';
import { watchUiRelease } from '../lib/ui-release';

export function useUiRelease(protectedInput: boolean, pendingAuth: boolean) {
  const latest = useRef({ protectedInput, pendingAuth });
  latest.current = { protectedInput, pendingAuth };
  const watcher = useRef<ReturnType<typeof watchUiRelease> | null>(null);
  const [available, setAvailable] = useState<string | null>(null);
  useEffect(() => {
    const loaded = document.querySelector<HTMLMetaElement>('meta[name="steward-ui-release"]')?.content;
    if (!loaded) return;
    watcher.current = watchUiRelease({
      loaded,
      protectedInput: () => latest.current.protectedInput,
      pendingAuth: () => latest.current.pendingAuth,
      ready: setAvailable,
      reload: () => window.location.reload(),
      confirm: () => window.confirm('刷新将丢弃当前未应用的输入和复制内容，是否采用新版？'),
    });
    return () => { watcher.current?.stop(); watcher.current = null; };
  }, []);
  return { available, apply: () => watcher.current?.apply() };
}
