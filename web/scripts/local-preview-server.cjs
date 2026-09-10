'use strict';
// The existing .local/local-preview entry: local peers only, GET-only upstream, no auth forwarding.
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const os = require('node:os');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
async function startPreview(root) {
  const config = JSON.parse(fs.readFileSync(path.join(root, 'config.json'), 'utf8'));
  const local = new Set(['127.0.0.1', '::1', ...Object.values(os.networkInterfaces()).flat().filter(Boolean).map(a => a.address)]);
  const upstream = new URL(config.upstream);
  if (!local.has(config.bind) || !local.has(upstream.hostname) || upstream.protocol !== 'http:' || upstream.username || upstream.password || upstream.pathname !== '/' || upstream.search || upstream.hash
    || !Number.isInteger(config.port) || config.port < 0 || config.port > 65535 || ![1, 4].includes(config.apiContract)) throw new Error('Invalid local readonly preview configuration');
  const advertised = await (await fetch(new URL('/ui/status', upstream), { signal: AbortSignal.timeout(5000) })).json();
  if (advertised.apiContract !== config.apiContract) throw new Error('Preview UI/upstream API contract mismatch; refusing to listen');
  const files = Object.fromEntries(['index.html', 'app.js', 'style.css'].map(name => [name, fs.readFileSync(path.join(root, 'ui', name))]));
  const manifest = { packageFormat: 1, uiVersion: config.uiVersion || 'local-preview', requiredApiContract: config.apiContract, entry: 'index.html', files: Object.fromEntries(Object.entries(files).map(([name, bytes]) => [name, sha(bytes)])) };
  const release = sha(Buffer.from(JSON.stringify(manifest)));
  const requests = new Set();
  const security = { 'cache-control': 'no-store', 'x-content-type-options': 'nosniff', 'referrer-policy': 'no-referrer', 'content-security-policy': "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'" };
  let address, timer;
  const server = http.createServer((req, res) => {
    const send = (status, value, type = 'text/plain; charset=utf-8') => { res.writeHead(status, { ...security, 'content-type': type }); res.end(value); };
    const peer = req.socket.remoteAddress?.replace(/^::ffff:/, '');
    if (![config.bind, '127.0.0.1', '::1'].includes(peer)) return send(403, 'Local preview only');
    if (req.headers.host !== new URL(address).host) return send(403, 'Unexpected host');
    if ((req.headers.origin && req.headers.origin !== address) || req.headers['sec-fetch-site'] === 'cross-site') return send(403, 'Cross-origin preview access denied');
    if (req.method !== 'GET') return send(405, 'Readonly preview: GET only');
    const url = new URL(req.url, address);
    if (url.pathname === '/ui/status') return send(200, JSON.stringify({ packageFormat: 1, apiContract: config.apiContract, externalEnabled: true, error: null, uiVersion: manifest.uiVersion, release, preview: true, dataSource: config.dataSource }), 'application/json');
    if (url.pathname === '/') return send(200, files['index.html'].toString('utf8').replace('<head>', `<head><meta name="steward-ui-release" content="${release}">`).replace('href="/style.css"', `href="/ui/releases/${release}/style.css"`).replace('src="/app.js"', `src="/ui/releases/${release}/app.js"`), 'text/html; charset=utf-8');
    for (const name of ['app.js', 'style.css']) if ([`/${name}`, `/ui/releases/${release}/${name}`].includes(url.pathname)) return send(200, files[name], name.endsWith('.js') ? 'text/javascript; charset=utf-8' : 'text/css; charset=utf-8');
    if (!url.pathname.startsWith('/api/')) return send(404, 'Not found');
    if (req.headers['x-steward-ui-contract'] !== undefined && req.headers['x-steward-ui-contract'] !== String(config.apiContract)) return send(409, JSON.stringify({ ok: false, error: { code: 'UI_API_INCOMPATIBLE', message: 'Reload the compatible preview UI' } }), 'application/json');
    // A fixed local origin, GET only. Never forward cookies, tokens, Authorization or Origin.
    const proxy = http.request({ hostname: upstream.hostname, port: upstream.port, path: url.pathname + url.search, method: 'GET', headers: { 'X-Steward-UI-Contract': String(config.apiContract), Accept: req.headers.accept || 'application/json' } }, response => {
      res.writeHead(response.statusCode, { ...security, 'content-type': response.headers['content-type'] || 'application/json' }); response.pipe(res);
    });
    requests.add(proxy); proxy.on('close', () => requests.delete(proxy));
    proxy.on('error', () => { if (!res.headersSent) send(502, JSON.stringify({ ok: false, error: { code: 'PREVIEW_UPSTREAM_UNAVAILABLE', message: 'Readonly upstream unavailable; no retry or write' } }), 'application/json'); else res.destroy(); });
    proxy.setTimeout(url.pathname === '/api/events' ? 120000 : 20000, () => proxy.destroy());
    res.on('close', () => proxy.destroy()); proxy.end();
  });
  const close = async () => {
    clearInterval(timer);
    for (const req of requests) req.destroy();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  };
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(config.port, config.bind, resolve); });
  address = `http://${config.bind.includes(':') ? `[${config.bind}]` : config.bind}:${server.address().port}`;
  const state = { pid: process.pid, url: address, upstream: config.upstream, apiContract: config.apiContract, dataSource: config.dataSource, uiVersion: manifest.uiVersion, release, files: manifest.files, stopFile: path.join(root, 'stop'), startedAt: new Date().toISOString(), mode: 'local-only GET preview; no credentials forwarded' };
  fs.writeFileSync(path.join(root, 'state.json'), JSON.stringify(state, null, 2));
  timer = setInterval(() => { if (fs.existsSync(state.stopFile)) void close(); }, 500);
  return { state, close };
}
module.exports = { startPreview };
if (require.main === module) startPreview(__dirname).then(({ state }) => console.log('Preview ready:', state.url)).catch(error => { console.error(error.message); process.exitCode = 1; });
