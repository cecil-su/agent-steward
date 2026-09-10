import type { Warning } from './contracts';

export class ApiError extends Error {
  constructor(message: string, readonly code: string, readonly uncertain = false, readonly details?: unknown) {
    super(message);
    this.name = 'ApiError';
  }
}
interface ApiOptions {
  fetch?: typeof fetch;
  onUnauthorized?: () => void;
  onWarnings?: (warnings: Warning[]) => void;
  getGeneration?: () => number;
}
function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

// No automatic retries. Aborting a POST cannot prove the server did not execute it.
export function createApi(options: ApiOptions = {}) {
  async function request<T>(path: string, body?: unknown, signal?: AbortSignal, token?: string, connectionCode?: string): Promise<T> {
    if (!path.startsWith('/api/') || /[\\\r\n#]/.test(path)) {
      throw new ApiError('只允许同源 API 路径。', 'INVALID_API_PATH');
    }
    const write = body !== undefined;
    const generation = options.getGeneration?.();
    const controller = new AbortController();
    const checkCurrent = () => {
      if (controller.signal.aborted || generation !== options.getGeneration?.()) {
        throw new ApiError('请求已取消或连接已切换。', 'STALE_RESPONSE', write);
      }
    };
    const abort = () => controller.abort();
    signal?.addEventListener('abort', abort, { once: true });
    if (signal?.aborted) abort();
    const timer = setTimeout(abort, 15_000);
    try {
      let response: Response;
      try {
        response = await (options.fetch ?? fetch)(path, {
          method: write ? 'POST' : 'GET',
          credentials: 'same-origin',
          cache: 'no-store',
          redirect: 'error',
          headers: {
            'X-Steward-UI-Contract': '3',
            ...(write ? { 'Content-Type': 'application/json', 'X-Steward-CSRF': '1' } : {}),
            ...(token ? { 'X-Steward-Token': token } : {}),
            ...(connectionCode ? { 'X-Steward-Connect': connectionCode } : {}),
          },
          body: write ? JSON.stringify(body) : undefined,
          signal: controller.signal,
        });
      } catch {
        throw new ApiError(write ? '结果未确认：请核对任务和现场，不要直接重复提交。' : '无法读取服务，请检查连接。', 'NETWORK_ERROR', write);
      }
      checkCurrent();
      // Clear data even when an expired-grant response has a malformed body.
      if (response.status === 401) options.onUnauthorized?.();
      let result: unknown;
      try { result = await response.json(); }
      catch { throw new ApiError('服务返回无法识别的结果，请刷新核对。', 'INVALID_RESPONSE', write); }
      checkCurrent();
      if (!isRecord(result) || typeof result.ok !== 'boolean') {
        throw new ApiError('服务返回无法识别的结果，请刷新核对。', 'INVALID_RESPONSE', write);
      }
      if (Array.isArray(result.warnings)) {
        options.onWarnings?.(result.warnings.filter((w): w is Warning => isRecord(w) && typeof w.code === 'string' && typeof w.message === 'string'));
      }
      if (!response.ok || !result.ok) {
        const error = isRecord(result.error) ? result.error : undefined;
        const code = typeof error?.code === 'string' ? error.code : String(response.status);
        const message = typeof error?.message === 'string' ? error.message : '请求失败';
        throw new ApiError(`${code}：${message}`, code, write && (!error || ['PARTIAL_EXTERNAL_STATE', 'INTERNAL_ERROR'].includes(code)), error?.details);
      }
      if (!Object.hasOwn(result, 'data')) throw new ApiError('响应缺少 data。', 'INVALID_RESPONSE', write);
      return result.data as T;
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
    }
  }
  return {
    get: <T>(path: string, signal?: AbortSignal) => request<T>(path, undefined, signal),
    // Business mutations deliberately have no frontend transport entry point.
    connect: (code: string, signal?: AbortSignal) => request<unknown>('/api/connect', {}, signal, undefined, code),
    login: (token: string) => request<unknown>('/api/login', {}, undefined, token),
    logout: () => request<unknown>('/api/logout', {}),
  };
}
