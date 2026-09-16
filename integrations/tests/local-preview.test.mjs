import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer, request } from 'node:http';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import preview from '../../web/scripts/local-preview-server.cjs';
const testBind = process.env.STEWARD_TEST_BIND || '127.0.0.1';

test('shared preview enforces contract, exact Host/Origin, local peers and GET-only credential-free proxy', async () => {
  const root = mkdtempSync(join(tmpdir(), 'steward-preview-contract-'));
  const forwarded = [];
  let contract = 5;
  const upstream = createServer((req, res) => {
    if (req.url === '/ui/status') { res.end(JSON.stringify({ apiContract: contract })); return; }
    forwarded.push({ method: req.method, headers: req.headers, url: req.url });
    res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ ok: true, data: { sessionRules: { formatVersion: 1, rules: [] } } }));
  });
  await new Promise((resolve, reject) => { upstream.once('error', reject); upstream.listen(0, testBind, resolve); });
  let instance;
  try {
    mkdirSync(join(root, 'ui'));
    const files = {
      'index.html': '<html><head></head><script src="/app.js"></script></html>',
      'app.js': 'fetch("/api/tasks/1/context", { headers: { "X-Steward-UI-Contract": "5" } });',
      'style.css': '/* synthetic */',
    };
    for (const [file, content] of Object.entries(files)) writeFileSync(join(root, 'ui', file), content);
    writeFileSync(join(root, 'config.json'), JSON.stringify({ bind: testBind, port: 0, upstream: `http://${testBind}:${upstream.address().port}`, apiContract: 5, dataSource: 'synthetic' }));
    contract = 1;
    await assert.rejects(preview.startPreview(root), /contract mismatch/);
    contract = 5;
    instance = await preview.startPreview(root);
    const url = instance.state.url;
    async function send(method = 'GET', headers = {}) {
      return new Promise((resolve, reject) => {
        const req = request(`${url}/api/tasks/1/context`, { method, headers }, res => { res.resume(); res.on('end', () => resolve(res.statusCode)); });
        req.on('error', reject); req.end();
      });
    }
    assert.equal(await send('POST'), 405);
    assert.equal(await send('PUT'), 405);
    assert.equal(await send('GET', { host: 'wrong.invalid' }), 403);
    assert.equal(await send('GET', { origin: 'http://wrong.invalid' }), 403);
    assert.equal(await send('GET', { 'sec-fetch-site': 'cross-site' }), 403);
    assert.equal(await send('GET', { 'X-Steward-UI-Contract': '1' }), 409);
    assert.equal(forwarded.length, 0);
    assert.equal(await send('GET', { origin: url, authorization: 'synthetic-do-not-forward', cookie: 'synthetic-cookie', 'x-steward-token': 'synthetic-token', 'X-Steward-UI-Contract': '5' }), 200);
    assert.equal(forwarded.length, 1);
    assert.equal(forwarded[0].method, 'GET');
    assert.equal(forwarded[0].headers['x-steward-ui-contract'], '5');
    for (const name of ['authorization', 'cookie', 'origin', 'x-steward-token']) assert.equal(forwarded[0].headers[name], undefined);
    assert.equal((await (await fetch(`${url}/ui/status`)).json()).dataSource, 'synthetic');
    assert.match(await (await fetch(url)).text(), /\/ui\/releases\//);
  } finally { if (instance) await instance.close(); await new Promise(resolve => upstream.close(resolve)); rmSync(root, { recursive: true, force: true }); }
});
