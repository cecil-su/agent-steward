import { afterEach, expect, it, vi } from 'vitest';
import { watchUiRelease } from './ui-release';

afterEach(() => vi.useRealTimers());
function fixture() {
  let current = 'new', dirty = false, auth = false;
  const ready = vi.fn(), reload = vi.fn(), confirm = vi.fn(() => true);
  const transport = vi.fn<typeof fetch>().mockImplementation(async () => new Response(JSON.stringify({ packageFormat: 1, apiContract: 1, release: current })));
  const start = () => watchUiRelease({ loaded: 'old', protectedInput: () => dirty, pendingAuth: () => auth, ready, reload, confirm, fetch: transport });
  return { ready, reload, confirm, start, dirty: (value: boolean) => { dirty = value; }, auth: (value: boolean) => { auth = value; }, release: (value: string) => { current = value; } };
}
it('reloads an idle page to a new release', async () => {
  vi.useFakeTimers(); const f = fixture(); const watcher = f.start();
  await vi.advanceTimersByTimeAsync(0);
  expect(f.reload).toHaveBeenCalledTimes(1); watcher.stop();
});
it('latches deferred updates until explicit adoption, even after search is applied', async () => {
  vi.useFakeTimers(); const f = fixture(); f.dirty(true); const watcher = f.start();
  await vi.advanceTimersByTimeAsync(0);
  expect(f.ready).toHaveBeenCalledWith('new'); expect(f.reload).not.toHaveBeenCalled();
  f.dirty(false); await vi.advanceTimersByTimeAsync(10_000);
  expect(f.reload).not.toHaveBeenCalled();
  f.auth(true); expect(watcher.apply()).toBe(false); expect(f.confirm).not.toHaveBeenCalled();
  f.auth(false); expect(watcher.apply()).toBe(true); expect(f.reload).toHaveBeenCalledTimes(1); watcher.stop();
});
it('clears the banner when the server rolls back to the loaded release', async () => {
  vi.useFakeTimers(); const f = fixture(); f.dirty(true); const watcher = f.start();
  await vi.advanceTimersByTimeAsync(0);
  f.release('old'); await vi.advanceTimersByTimeAsync(10_000);
  expect(f.ready).toHaveBeenLastCalledWith(null); expect(watcher.apply()).toBe(false); watcher.stop();
});
it('does not act on a late status response after cleanup', async () => {
  let finish!: (response: Response) => void;
  const reload = vi.fn();
  const transport = vi.fn<typeof fetch>().mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const watcher = watchUiRelease({ loaded: 'old', protectedInput: () => false, pendingAuth: () => false, ready: vi.fn(), reload, confirm: () => true, fetch: transport });
  watcher.stop(); finish(new Response(JSON.stringify({ packageFormat: 1, apiContract: 1, release: 'new' })));
  await Promise.resolve(); await Promise.resolve(); expect(reload).not.toHaveBeenCalled();
});
