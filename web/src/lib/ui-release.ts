interface ReleaseOptions {
  loaded: string;
  protectedInput: () => boolean;
  pendingAuth: () => boolean;
  ready: (release: string | null) => void;
  reload: () => void;
  confirm: () => boolean;
  fetch?: typeof fetch;
}

export function watchUiRelease(options: ReleaseOptions) {
  let active = true;
  let held = false;
  let candidate: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let request: AbortController | undefined;
  async function check() {
    request = new AbortController();
    const timeout = setTimeout(() => request?.abort(), 5000);
    try {
      const response = await (options.fetch ?? fetch)('/ui/status', { credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal: request.signal });
      if (!response.ok || !active) return;
      const status = await response.json();
      if (!active || !status || status.packageFormat !== 1 || !Number.isInteger(status.apiContract) || typeof status.release !== 'string') return;
      if (status.release === options.loaded) {
        candidate = null; held = false; options.ready(null); return;
      }
      candidate = status.release;
      if (!held && !options.protectedInput() && !options.pendingAuth()) { options.reload(); return; }
      held = true;
      options.ready(candidate);
    } catch { /* A failed UI status read must not disturb browsing. */ }
    finally { clearTimeout(timeout); if (active) timer = setTimeout(check, 10_000); }
  }
  void check();
  return {
    stop: () => { active = false; clearTimeout(timer); request?.abort(); },
    apply: () => {
      if (!active || !candidate || options.pendingAuth() || !options.confirm()) return false;
      options.reload(); return true;
    },
  };
}
