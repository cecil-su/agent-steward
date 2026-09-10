import { afterEach, expect, it, vi } from 'vitest';
import { subscribeLiveEvents } from './live-events';

const callbacks = () => ({ changed: vi.fn(), unauthorized: vi.fn(), status: vi.fn(), revalidate: vi.fn().mockResolvedValue(undefined) });
function stream() {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const response = new Response(new ReadableStream<Uint8Array>({ start(value) { controller = value; } }));
  return { response, send: (text: string) => controller.enqueue(new TextEncoder().encode(text)) };
}
afterEach(() => vi.useRealTimers());
it('parses split frames, sends the contract header and closes on cleanup', async () => {
  const input = stream(), cb = callbacks();
  const transport = vi.fn<typeof fetch>().mockResolvedValue(input.response);
  const stop = subscribeLiveEvents(cb, transport);
  await vi.waitFor(() => expect(cb.status).toHaveBeenCalledWith('实时同步'));
  input.send('event: chan'); input.send('ged\r\ndata: refresh\r\n\r\n');
  await vi.waitFor(() => expect(cb.changed).toHaveBeenCalledTimes(1));
  expect(transport.mock.calls[0][1]).toMatchObject({ method: 'GET', headers: { 'X-Steward-UI-Contract': '3' }, credentials: 'same-origin' });
  stop(); expect(transport.mock.calls[0][1]?.signal?.aborted).toBe(true);
});
it('closes revoked streams without replaying authentication', async () => {
  const input = stream(), cb = callbacks();
  const transport = vi.fn<typeof fetch>().mockResolvedValue(input.response);
  const stop = subscribeLiveEvents(cb, transport);
  await vi.waitFor(() => expect(cb.status).toHaveBeenCalledWith('实时同步'));
  input.send('event: unauthorized\n\n');
  await vi.waitFor(() => expect(cb.unauthorized).toHaveBeenCalledTimes(1));
  expect(transport).toHaveBeenCalledTimes(1); stop();
});
it('rebuilds GET streams after HTTP 503 and cancels further retries on cleanup', async () => {
  vi.useFakeTimers(); const input = stream(), cb = callbacks();
  const transport = vi.fn<typeof fetch>().mockResolvedValueOnce(new Response('', { status: 503 })).mockResolvedValue(input.response);
  const stop = subscribeLiveEvents(cb, transport);
  await vi.advanceTimersByTimeAsync(0);
  expect(cb.revalidate).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(2000);
  expect(transport).toHaveBeenCalledTimes(2);
  expect(cb.status).toHaveBeenCalledWith('实时同步');
  stop(); await vi.advanceTimersByTimeAsync(5000);
  expect(transport).toHaveBeenCalledTimes(2);
});
it('ignores late 401 responses after subscription cleanup', async () => {
  const cb = callbacks(); let finish!: (response: Response) => void;
  const transport = vi.fn<typeof fetch>().mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const stop = subscribeLiveEvents(cb, transport);
  stop(); finish(new Response('', { status: 401 }));
  await Promise.resolve(); await Promise.resolve(); expect(cb.unauthorized).not.toHaveBeenCalled();
});
