import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useLiveUpdates } from './use-live-updates';

function mockStreams() {
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  vi.stubGlobal('fetch', vi.fn<typeof fetch>().mockImplementation(async () => new Response(new ReadableStream<Uint8Array>({ start(controller) { streams.push(controller); } }))));
  return { changed: (index = streams.length - 1) => streams[index].enqueue(new TextEncoder().encode('event: changed\n\n')) };
}
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
it('keeps search/copy input while updates arrive and refreshes after it is applied', async () => {
  vi.useFakeTimers(); const streams = mockStreams();
  const refresh = vi.fn().mockResolvedValue(undefined);
  const options = { refresh, unauthorized: vi.fn(), revalidate: vi.fn().mockResolvedValue(undefined) };
  const hook = renderHook(({ protectedInput }) => useLiveUpdates(true, { ...options, protectedInput }), { initialProps: { protectedInput: true } });
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(async () => streams.changed());
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(refresh).not.toHaveBeenCalled(); expect(hook.result.current).toContain('当前输入已保留');
  hook.rerender({ protectedInput: false });
  await act(() => vi.advanceTimersByTimeAsync(250));
  expect(refresh).toHaveBeenCalledTimes(1); hook.unmount();
});
it('coalesces high-frequency changes while a refresh is pending', async () => {
  vi.useFakeTimers(); const streams = mockStreams();
  let finish!: () => void;
  const refresh = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; })).mockResolvedValue(undefined);
  const hook = renderHook(() => useLiveUpdates(true, { refresh, protectedInput: false, unauthorized: vi.fn(), revalidate: vi.fn().mockResolvedValue(undefined) }));
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(async () => streams.changed()); await act(() => vi.advanceTimersByTimeAsync(250));
  for (let i = 0; i < 3; i++) { await act(async () => streams.changed()); await act(() => vi.advanceTimersByTimeAsync(1000)); }
  expect(refresh).toHaveBeenCalledTimes(1);
  await act(async () => finish()); await act(() => vi.advanceTimersByTimeAsync(250));
  expect(refresh).toHaveBeenCalledTimes(2); hook.unmount();
});
it('does not let a rejected old refresh overwrite a new connection status', async () => {
  vi.useFakeTimers(); const streams = mockStreams();
  let reject!: (error: Error) => void;
  const refresh = vi.fn().mockImplementationOnce(() => new Promise<void>((_resolve, fail) => { reject = fail; })).mockResolvedValue(undefined);
  const hook = renderHook(({ enabled }) => useLiveUpdates(enabled, { refresh, protectedInput: false, unauthorized: vi.fn(), revalidate: vi.fn().mockResolvedValue(undefined) }), { initialProps: { enabled: true } });
  await act(() => vi.advanceTimersByTimeAsync(0));
  await act(async () => streams.changed()); await act(() => vi.advanceTimersByTimeAsync(250));
  hook.rerender({ enabled: false }); hook.rerender({ enabled: true });
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(hook.result.current).toBe('实时同步');
  await act(async () => reject(new Error('old')));
  expect(hook.result.current).toBe('实时同步'); hook.unmount();
});
