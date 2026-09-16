import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import preview from '../../web/scripts/local-preview-server.cjs';

const { startPreview } = preview;
const bind = process.env.STEWARD_TEST_BIND || '127.0.0.1';
test('preview refuses missing, mixed and stale UI contracts before listening', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'steward-preview-resource-test-'));
  let instance;
  let contract = 5;
  const upstream = http.createServer((req, res) => {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ apiContract: contract }));
  });
  try {
    await new Promise((resolve, reject) => { upstream.once('error', reject); upstream.listen(0, bind, resolve); });
    fs.mkdirSync(path.join(root, 'ui'));
    fs.writeFileSync(path.join(root, 'ui/index.html'), '<html><head></head><script src="/app.js"></script></html>');
    fs.writeFileSync(path.join(root, 'ui/style.css'), 'body{}');
    const configure = apiContract => fs.writeFileSync(path.join(root, 'config.json'), JSON.stringify({ bind, port: 0, upstream: `http://${bind}:${upstream.address().port}`, apiContract }));
    const script = content => fs.writeFileSync(path.join(root, 'ui/app.js'), content);
    configure(5);
    for (const content of ['synthetic', 'const h={"X-Steward-UI-Contract":"4"};', 'const h={"X-Steward-UI-Contract":"5"},s={"X-Steward-UI-Contract":"4"};']) {
      script(content);
      await assert.rejects(startPreview(root), /resource\/config API contract mismatch/);
      assert.equal(fs.existsSync(path.join(root, 'state.json')), false);
    }
    script('const h={"X-Steward-UI-Contract":`5`};');
    contract = 4;
    await assert.rejects(startPreview(root), /UI\/upstream API contract mismatch/);
    contract = 5;
    instance = await startPreview(root);
    assert.equal((await (await fetch(instance.state.url + '/ui/status')).json()).apiContract, 5);
    assert.equal(await (await fetch(instance.state.url + '/app.js')).text(), 'const h={"X-Steward-UI-Contract":`5`};');
    await instance.close(); instance = null;
    for (const legacy of [1, 4]) {
      contract = legacy; configure(legacy);
      script(`const h={'X-Steward-UI-Contract':'${legacy}'};`);
      instance = await startPreview(root);
      assert.equal(instance.state.apiContract, legacy);
      await instance.close(); instance = null;
    }
  } finally {
    if (instance) await instance.close();
    await new Promise(resolve => upstream.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
});
