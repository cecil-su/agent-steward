// @vitest-environment node
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { checkDist } from './check-dist.mjs';
let directory;
const html = '<html><head><link rel="stylesheet" href="/style.css"></head><body><script type="module" src="/app.js"></script></body></html>';
beforeEach(async () => {
  directory = await mkdtemp(join(tmpdir(), 'steward-web-contract-'));
  await Promise.all(Object.entries({ 'index.html': html, 'app.js': 'console.log("fixture");', 'style.css': 'body{margin:0}' }).map(([name, content]) => writeFile(join(directory, name), content)));
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });
it('accepts exact three-file resources', async () => { await expect(checkDist(directory)).resolves.toEqual(['app.js', 'index.html', 'style.css']); });
it('rejects chunks', async () => { await writeFile(join(directory, 'chunk.js'), 'extra'); await expect(checkDist(directory)).rejects.toThrow('exactly three'); });
it('rejects missing pinned-entry markers', async () => { await writeFile(join(directory, 'index.html'), html.replace('/app.js', '/assets/app.js')); await expect(checkDist(directory)).rejects.toThrow('entry markers'); });
it('rejects invalid UTF-8', async () => { await writeFile(join(directory, 'app.js'), Buffer.from([0xff])); await expect(checkDist(directory)).rejects.toThrow(); });
it('rejects inline scripts', async () => { await writeFile(join(directory, 'index.html'), html.replace('</body>', '<script>alert(1)</script></body>')); await expect(checkDist(directory)).rejects.toThrow('external app.js'); });
it('rejects single-quoted and srcset resource attributes', async () => {
  for (const tag of ["<img src='/missing.png'>", '<img srcset="/missing.png 2x">']) {
    await writeFile(join(directory, 'index.html'), html.replace('</body>', tag + '</body>'));
    await expect(checkDist(directory)).rejects.toThrow('Unexpected HTML resource');
  }
});
it('rejects static and dynamic JS imports', async () => {
  for (const js of ['import "./missing.js";', 'import("./missing.js");']) {
    await writeFile(join(directory, 'app.js'), js);
    await expect(checkDist(directory)).rejects.toThrow('additional modules');
  }
});
it('rejects ordinary worker constructors', async () => {
  for (const js of ['new Worker("/missing.js");', 'new window.SharedWorker("/missing.js");']) {
    await writeFile(join(directory, 'app.js'), js);
    await expect(checkDist(directory)).rejects.toThrow('Workers are not supported');
  }
});
it('rejects stylesheet resource dependencies', async () => { await writeFile(join(directory, 'style.css'), 'body{background:url(/unmanaged.png)}'); await expect(checkDist(directory)).rejects.toThrow('additional resources'); });
