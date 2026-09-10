interface LiveCallbacks {
  changed: () => void;
  unauthorized: () => void;
  status: (status: string) => void;
  revalidate: () => Promise<void>;
}

// Explicit GET-only reconnect, including HTTP 503; all requests carry the UI contract.
export function subscribeLiveEvents(callbacks: LiveCallbacks, transport: typeof fetch = fetch) {
  const controller = new AbortController();
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  const pause = () => new Promise<void>((resolve) => {
    const finish = () => { clearTimeout(timer); controller.signal.removeEventListener('abort', finish); resolve(); };
    const timer = setTimeout(finish, 2000);
    controller.signal.addEventListener('abort', finish, { once: true });
    if (controller.signal.aborted) finish();
  });
  const unauthorized = () => {
    if (controller.signal.aborted) return;
    controller.abort(); callbacks.unauthorized();
  };
  void (async () => {
    while (!controller.signal.aborted) {
      callbacks.status('正在连接实时更新…');
      try {
        const response = await transport('/api/events', { method: 'GET', headers: { 'X-Steward-UI-Contract': '3' }, credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal: controller.signal });
        if (controller.signal.aborted) { await response.body?.cancel().catch(() => {}); return; }
        if (response.status === 401) { unauthorized(); return; }
        if (!response.ok || !response.body) throw new Error('Stream unavailable');
        callbacks.status('实时同步');
        reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        while (!controller.signal.aborted) {
          const { done, value } = await reader.read();
          if (controller.signal.aborted || done) break;
          buffer += decoder.decode(value, { stream: true }).replace(/\r/g, '');
          let end;
          while ((end = buffer.indexOf('\n\n')) !== -1) {
            if (end > 8192) throw new Error('Oversize event');
            const frame = buffer.slice(0, end); buffer = buffer.slice(end + 2);
            const events = frame.split('\n').filter((line) => line.startsWith('event:')).map((line) => line.slice(6).trim());
            if (events.includes('unauthorized')) { unauthorized(); return; }
            if (events.includes('unavailable')) throw new Error('Stream unavailable');
            if (events.includes('changed')) callbacks.changed();
          }
          if (buffer.length > 8192) throw new Error('Oversize event');
        }
      } catch { /* Retry only the read-only notification channel. */ }
      finally { if (reader) { await reader.cancel().catch(() => {}); reader = undefined; } }
      if (controller.signal.aborted) return;
      callbacks.status('同步断开，正在重连；可手动刷新');
      await callbacks.revalidate().catch(() => {});
      if (!controller.signal.aborted) await pause();
    }
  })();
  return () => { controller.abort(); void reader?.cancel().catch(() => {}); };
}
