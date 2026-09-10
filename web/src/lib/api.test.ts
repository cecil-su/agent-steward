import { describe, expect, it, vi } from 'vitest';
import { createApi } from './api';

const reply = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
describe('same-origin transport', () => {
  it('sends contract headers and never stores credentials', async () => {
    const transport = vi.fn<typeof fetch>().mockImplementation(async () => reply({ ok: true, data: { id: 1 } }));
    const api = createApi({ fetch: transport });
    await expect(api.get('/api/tasks/1')).resolves.toEqual({ id: 1 });
    expect(transport.mock.calls[0][1]).toMatchObject({ method: 'GET', credentials: 'same-origin', cache: 'no-store', redirect: 'error', headers: { 'X-Steward-UI-Contract': '4' } });
    await api.login('synthetic-credential');
    expect(transport.mock.calls[1][1]).toMatchObject({ method: 'POST', headers: { 'X-Steward-CSRF': '1', 'X-Steward-Token': 'synthetic-credential' }, body: '{}' });
  });
  it('refuses external URLs before fetching', async () => {
    const transport = vi.fn<typeof fetch>();
    for (const path of ['https://example.invalid/api/tasks', '//example.invalid/api/tasks', '/api/\\evil']) {
      await expect(createApi({ fetch: transport }).get(path)).rejects.toMatchObject({ code: 'INVALID_API_PATH' });
    }
    expect(transport).not.toHaveBeenCalled();
  });
  it('exposes only reads and authentication, not business commands', () => {
    expect(Object.keys(createApi()).sort()).toEqual(['connect', 'get', 'login', 'logout']);
  });
  it('does not retry uncertain logout or expose transport error/credentials', async () => {
    const transport = vi.fn<typeof fetch>().mockRejectedValue(new Error('sensitive transport text'));
    await expect(createApi({ fetch: transport }).logout()).rejects.toMatchObject({ code: 'NETWORK_ERROR', uncertain: true });
    expect(transport).toHaveBeenCalledTimes(1);
  });
  it('preserves structured read errors and details', async () => {
    const transport = vi.fn<typeof fetch>().mockResolvedValue(reply({ ok: false, error: { code: 'VERSION_CONFLICT', message: 'stale', details: { currentRevision: 4 } } }, 409));
    await expect(createApi({ fetch: transport }).get('/api/projects/1')).rejects.toMatchObject({ code: 'VERSION_CONFLICT', uncertain: false, details: { currentRevision: 4 } });
    expect(transport).toHaveBeenCalledTimes(1);
  });
  it('reports malformed, partial and internal write outcomes as uncertain', async () => {
    for (const response of [new Response('not json'), reply({}), reply({ ok: true }), reply({ ok: false, error: { code: 'PARTIAL_EXTERNAL_STATE' } }, 500), reply({ ok: false, error: { code: 'INTERNAL_ERROR' } }, 500)]) {
      const transport = vi.fn<typeof fetch>().mockResolvedValue(response);
      await expect(createApi({ fetch: transport }).logout()).rejects.toMatchObject({ uncertain: true });
    }
  });
  it('clears authorization even if a 401 body cannot be decoded', async () => {
    const onUnauthorized = vi.fn();
    const transport = vi.fn<typeof fetch>().mockResolvedValue(new Response('expired', { status: 401 }));
    await expect(createApi({ fetch: transport, onUnauthorized }).get('/api/access')).rejects.toThrow();
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });
  it('ignores a late unauthorized response after query cancellation', async () => {
    const onUnauthorized = vi.fn();
    let finish!: (response: Response) => void;
    const transport = vi.fn<typeof fetch>().mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const controller = new AbortController();
    const result = createApi({ fetch: transport, onUnauthorized }).get('/api/access', controller.signal);
    controller.abort();
    finish(new Response('expired', { status: 401 }));
    await expect(result).rejects.toMatchObject({ code: 'STALE_RESPONSE' });
    expect(onUnauthorized).not.toHaveBeenCalled();
  });
  it('ignores old-connection warnings after delayed JSON parsing', async () => {
    const onWarnings = vi.fn();
    let generation = 1;
    let finish!: (body: unknown) => void;
    const response = reply({});
    vi.spyOn(response, 'json').mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const transport = vi.fn<typeof fetch>().mockResolvedValue(response);
    const result = createApi({ fetch: transport, onWarnings, getGeneration: () => generation }).get('/api/access');
    await vi.waitFor(() => expect(finish).toBeDefined());
    generation++;
    finish({ ok: true, data: {}, warnings: [{ code: 'OLD', message: 'old connection' }] });
    await expect(result).rejects.toMatchObject({ code: 'STALE_RESPONSE' });
    expect(onWarnings).not.toHaveBeenCalled();
  });
  it('forwards only well-formed warnings', async () => {
    const onWarnings = vi.fn();
    const transport = vi.fn<typeof fetch>().mockResolvedValue(reply({ ok: true, data: {}, warnings: [{ code: 'OBSERVATION', message: 'not verified' }, null] }));
    await createApi({ fetch: transport, onWarnings }).get('/api/access');
    expect(onWarnings).toHaveBeenCalledWith([{ code: 'OBSERVATION', message: 'not verified' }]);
  });
});
