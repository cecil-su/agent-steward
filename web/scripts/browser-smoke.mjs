import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { chromium } from 'playwright';
import { checkDist } from './check-dist.mjs';

// Explicit development binaries and synthetic state only. No installed binary/URL fallback.
const binDir = process.env.STEWARD_TEST_BIN_DIR;
const bind = process.env.STEWARD_TEST_BIND ?? '127.0.0.1';
const localAuth = process.env.STEWARD_TEST_LOCAL_AUTH === '1';
assert(binDir && path.isAbsolute(binDir), 'Set STEWARD_TEST_BIN_DIR to an absolute development binary directory');
assert(Object.values(os.networkInterfaces()).flat().some((entry) => entry?.family === 'IPv4' && entry.address === bind), 'Bind must be a local IPv4 address');
const binary = (name) => path.join(binDir, name + (process.platform === 'win32' ? '.exe' : ''));
for (const name of ['taskd', 'taskctl']) fs.accessSync(binary(name), fs.constants.X_OK);
const dist = path.resolve('dist');
await checkDist(dist);
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'steward-react-smoke-'));
const stop = path.join(temp, 'stop');
let child, browser, startupError;
let succeeded = false;
let exited = Promise.resolve();
try {
  const database = path.join(temp, 'synthetic.db');
  const runtime = path.join(temp, 'runtime');
  const uiRoot = path.join(temp, 'ui');
  const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
  const resources = Object.fromEntries(['index.html', 'app.js', 'style.css'].map((name) => [name, fs.readFileSync(path.join(dist, name))]));
  const manifest = Buffer.from(JSON.stringify({ packageFormat: 1, uiVersion: 'react-smoke', requiredApiContract: 2, entry: 'index.html', files: Object.fromEntries(Object.entries(resources).map(([name, bytes]) => [name, sha(bytes)])) }));
  let release = sha(manifest);
  const embedded = process.env.STEWARD_TEST_UI_MODE === 'embedded';
  const releaseDir = path.join(uiRoot, 'releases', release);
  fs.mkdirSync(releaseDir, { recursive: true });
  fs.writeFileSync(path.join(releaseDir, 'manifest.json'), manifest);
  for (const [name, bytes] of Object.entries(resources)) fs.writeFileSync(path.join(releaseDir, name), bytes);
  fs.writeFileSync(path.join(uiRoot, 'current.json'), JSON.stringify({ release: embedded ? 'embedded' : release }));
  const cli = (...args) => {
    const result = JSON.parse(execFileSync(binary('taskctl'), ['--database', database, '--json', ...args], { encoding: 'utf8' }));
    assert.equal(result.ok, true); return result.data;
  };
  const project = cli('project', 'create', '--name', 'React隔离项目').project;
  const task = cli('task', 'create', 'REACT-SMOKE', '--project', `##${project.id}`).task;
  let current = cli('task', 'claim', String(task.id), '--session', 'react-read-session', '--if-version', String(task.version)).task;
  const checkpointFile = path.join(temp, 'checkpoint.json');
  fs.writeFileSync(checkpointFile, JSON.stringify({ summary: '隔离Checkpoint摘要', completed: ['隔离数据准备'], decisions: [], pending: [], risks: [], nextStep: '验证只读页面' }));
  current = cli('task', 'checkpoint', String(task.id), '--session', 'react-read-session', '--if-version', String(current.version), '--input', checkpointFile).task;
  cli('task', 'note', String(task.id), '--if-version', String(current.version), '--type', 'progress', '--text', '可验证的只读备注');
  const baseline = cli('task', 'show', String(task.id)).task;
  const baselineHistory = cli('history', String(task.id)).history;
  const profileFile = path.join(temp, 'profile.json');
  fs.writeFileSync(profileFile, JSON.stringify({ summary: '独立项目资料简介', architecture: '隔离架构与入口', development: '隔离开发验证方式', evidence: '合成来源依据，不是实时服务状态', sourceTaskId: task.id, sourceTaskVersion: baseline.version }));
  cli('project', 'profile', 'set', `##${project.id}`, '--if-revision', String(project.revision), '--input', profileFile);
  const profileBaseline = cli('project', 'show', `##${project.id}`);
  const projectHistoryBaseline = cli('project', 'history', `##${project.id}`);
  child = spawn(binary('taskd'), ['--database', database, '--runtime-dir', runtime, '--ui-root', uiRoot, '--shutdown-file', stop, ...(localAuth ? [] : ['--require-local-auth']), '--bind', bind, '--port', '0', '--no-open'], { stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  child.stdout.on('data', (bytes) => { stdout += bytes; }); child.stderr.resume();
  exited = new Promise((resolve) => {
    child.once('exit', resolve);
    child.once('error', (error) => { startupError = error; resolve(); });
  });
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  for (let i = 0; i < 200 && !stdout.includes('Credential file:'); i++) {
    if (startupError) throw new Error('Unable to start isolated taskd', { cause: startupError });
    assert(child.exitCode === null && child.signalCode === null, 'Isolated taskd exited during startup');
    await delay(50);
  }
  const url = stdout.match(/Agent Steward: (http:\/\/[\d.]+:\d+)/)?.[1];
  const credentialPath = stdout.match(/Credential file: (.+)/)?.[1]?.trim();
  assert(url && credentialPath, 'Isolated taskd did not advertise startup');
  const reader = fs.readFileSync(path.join(path.dirname(credentialPath), 'readonly-credential'), 'utf8').trim();
  const status = await fetch(url + '/ui/status').then((response) => response.json());
  if (embedded) {
    assert(/^embedded-[a-f0-9]{64}$/.test(status.release), 'Readonly embedded UI was not selected');
    release = status.release;
  } else assert(JSON.stringify(status).includes(release), 'Candidate UI was not adopted by the isolated daemon');
  browser = await chromium.launch({ headless: true, channel: process.env.STEWARD_BROWSER_CHANNEL === 'chromium' ? undefined : process.env.STEWARD_BROWSER_CHANNEL ?? 'chrome' });
  const context = await browser.newContext({ viewport: { width: 1360, height: 1000 } });
  const page = await context.newPage();
  const pageErrors = [], writes = [];
  let subscriptions = 0;
  await page.route('**/api/events', async (route) => {
    subscriptions++;
    assert.equal(route.request().headers()['x-steward-ui-contract'], '2');
    if (subscriptions === 1) await route.fulfill({ status: 503, contentType: 'application/json', body: '{"ok":false,"error":{"code":"SERVER_BUSY"}}' });
    else await route.continue();
  });
  page.on('pageerror', (error) => pageErrors.push(error.message));
  page.on('request', (request) => { if (request.method() === 'POST') writes.push(new URL(request.url()).pathname); });
  await page.addInitScript(() => {
    window.__cspViolations = [];
    document.addEventListener('securitypolicyviolation', (event) => window.__cspViolations.push(event.violatedDirective));
  });
  const artifacts = path.resolve('.artifacts', new Date().toISOString().replaceAll(':', '-'));
  fs.mkdirSync(artifacts, { recursive: true });
  await page.goto(url);
  if (!localAuth) {
    await page.getByLabel('连接凭据（本机授权可留空）').fill(reader);
    await page.getByRole('button', { name: '连接', exact: true }).click();
  }
  await page.getByRole('button', { name: new RegExp(`^#${task.id}\\b`) }).waitFor();
  await page.getByText('实时同步', { exact: true }).waitFor();
  assert(subscriptions >= 2, 'SSE failed to reconnect after HTTP 503');
  await page.reload();
  await page.getByRole('button', { name: new RegExp(`^#${task.id}\\b`) }).waitFor();
  assert.equal(await page.getByRole('button', { name: '连接', exact: true }).count(), 0, 'Refresh did not restore cookie authorization');
  await page.getByRole('link', { name: 'Agent Steward · 本地任务工作台' }).waitFor();
  assert.deepEqual(await page.getByLabel('任务状态视图').getByRole('button').allTextContents(), ['未关闭', '进行中', '有阻塞', '已关闭', '最近全部']);
  assert.equal(await page.getByText(/项目资料和任务由 AI|Web 仅用于检索和展示/).count(), 0);
  await page.route('**/api/tasks?**', async route => {
    if (['in-progress', 'blocked'].includes(new URL(route.request().url()).searchParams.get('view'))) await delay(800);
    await route.continue();
  });
  for (const [width, name] of [[1360, '进行中'], [390, '有阻塞']]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.getByRole('button', { name: '刷新', exact: true }).waitFor();
    const anchors = [page.getByLabel('任务状态视图'), page.getByRole('heading', { name: '任务列表', exact: true }), page.getByRole('heading', { name: '任务详情', exact: true })];
    const before = await Promise.all(anchors.map(anchor => anchor.boundingBox()));
    await page.getByRole('button', { name, exact: true }).click();
    await page.getByText('正在加载…', { exact: true }).waitFor();
    assert.deepEqual(await Promise.all(anchors.map(anchor => anchor.boundingBox())), before, 'Loading moved workspace anchors');
    await page.getByRole('button', { name: '刷新', exact: true }).waitFor();
    assert.deepEqual(await Promise.all(anchors.map(anchor => anchor.boundingBox())), before, 'Completed filtering moved workspace anchors');
    await page.screenshot({ path: path.join(artifacts, `stable-filter-${width}.png`), fullPage: true });
    await page.getByRole('button', { name: '未关闭', exact: true }).click();
    await page.getByRole('button', { name: new RegExp(`^#${task.id}\\b`) }).waitFor();
  }
  await page.unroute('**/api/tasks?**');
  await page.setViewportSize({ width: 1360, height: 1000 });
  await page.getByRole('button', { name: new RegExp(`^#${task.id}\\b`) }).click();
  await page.getByRole('heading', { name: '未命名任务', exact: true }).waitFor();
  await page.getByText('隔离Checkpoint摘要', { exact: true }).waitFor();
  await page.getByText('独立项目资料简介', { exact: true }).waitFor();
  assert.equal(await page.getByRole('alert').count(), 0, 'Task context failed to load');
  await page.getByRole('button', { name: '进展备注', exact: true }).click();
  await page.getByText('可验证的只读备注', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Session', exact: true }).click();
  await page.getByText('react-read-session', { exact: true }).waitFor();
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await page.getByText('task.created', { exact: false }).first().waitFor();
  await page.getByRole('button', { name: '代码现场', exact: true }).click();
  await page.getByText('代码现场不可观察', { exact: true }).waitFor();
  await page.getByRole('button', { name: '复制上下文', exact: true }).click();
  const copied = await page.getByLabel('上下文（可手工复制）').inputValue();
  assert(copied.includes('隔离Checkpoint摘要'));
  assert(copied.includes('独立项目资料简介'));
  assert(copied.includes('"sourceTaskVersion": ' + baseline.version));
  await page.getByRole('button', { name: '关闭复制预览', exact: true }).click();
  await page.getByRole('button', { name: '概览', exact: true }).click();
  assert.equal(await page.getByRole('button', { name: /新建|编辑|改名|保存|关闭任务|关联项目|删除/ }).count(), 0, 'Business write controls are exposed');
  await page.getByText('实时同步', { exact: true }).waitFor();
  assert(subscriptions >= 2, 'SSE failed to reconnect after HTTP 503');
  await page.getByRole('searchbox').fill('未应用的搜索草稿');
  const extra = cli('task', 'create', 'SSE-READONLY').task;
  await page.getByText(/当前输入已保留/).waitFor();
  assert.equal(await page.getByRole('searchbox').inputValue(), '未应用的搜索草稿');
  assert.equal(await page.getByRole('button', { name: new RegExp(`^#${extra.id}\\b`) }).count(), 0);
  await page.getByRole('searchbox').fill('');
  await page.getByRole('button', { name: new RegExp(`^#${extra.id}\\b`) }).waitFor();
  await page.screenshot({ path: path.join(artifacts, 'tasks-desktop.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Tasks overflow on mobile');
  await page.screenshot({ path: path.join(artifacts, 'tasks-mobile.png'), fullPage: true });
  await page.getByRole('button', { name: '项目', exact: true }).click();
  await page.getByRole('searchbox').fill('React隔离项目');
  await page.getByRole('searchbox').press('Enter');
  await page.getByRole('button', { name: /React隔离项目/ }).click();
  await page.getByRole('heading', { name: 'React隔离项目', exact: true }).waitFor();
  await page.getByText('project.created', { exact: false }).first().waitFor();
  await page.getByText('独立项目资料简介', { exact: true }).waitFor();
  await page.getByText('合成来源依据，不是实时服务状态', { exact: true }).waitFor();
  await page.getByText('project.profile_updated', { exact: false }).first().waitFor();
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Project detail overflows on mobile');
  await page.screenshot({ path: path.join(artifacts, 'projects-mobile.png'), fullPage: true });
  await page.getByRole('button', { name: '查看关联任务', exact: true }).click();
  await page.getByRole('button', { name: new RegExp(`^#${task.id}\\b`) }).waitFor();
  assert.equal(await page.getByRole('button', { name: new RegExp(`^#${extra.id}\\b`) }).count(), 0, 'Project filter includes an unrelated task');
  await page.getByRole('searchbox').fill('保留新版提示前的输入');
  const nextManifest = Buffer.from(JSON.stringify({ ...JSON.parse(manifest.toString()), uiVersion: 'react-smoke-next' }));
  const nextRelease = sha(nextManifest);
  const nextDir = path.join(uiRoot, 'releases', nextRelease);
  fs.mkdirSync(nextDir);
  fs.writeFileSync(path.join(nextDir, 'manifest.json'), nextManifest);
  for (const [name, bytes] of Object.entries(resources)) fs.writeFileSync(path.join(nextDir, name), bytes);
  fs.writeFileSync(path.join(uiRoot, 'current.json'), JSON.stringify({ release: nextRelease }));
  await page.getByRole('button', { name: '刷新采用新版' }).waitFor({ timeout: 20_000 });
  assert.equal(await page.getByRole('searchbox').inputValue(), '保留新版提示前的输入');
  page.once('dialog', (dialog) => dialog.dismiss());
  await page.getByRole('button', { name: '刷新采用新版' }).click();
  assert.equal(await page.getByRole('searchbox').inputValue(), '保留新版提示前的输入');
  await page.getByRole('button', { name: '退出连接' }).click();
  await page.getByRole('heading', { name: '连接只读工作台' }).waitFor();
  assert.deepEqual(writes, localAuth ? ['/api/logout'] : ['/api/login', '/api/logout']);
  assert.deepEqual(await page.evaluate(() => window.__cspViolations), []);
  assert.deepEqual(pageErrors, []);
  assert.equal(await page.getByRole('alert').count(), 0);
  assert.deepEqual(cli('task', 'show', String(task.id)).task, baseline);
  assert.deepEqual(cli('history', String(task.id)).history, baselineHistory);
  assert.deepEqual(cli('project', 'show', `##${project.id}`), profileBaseline);
  assert.deepEqual(cli('project', 'history', `##${project.id}`), projectHistoryBaseline);
  assert.equal(await page.evaluate(() => localStorage.length), 0);
  assert.equal(await page.evaluate(() => sessionStorage.length), 0);
  console.log(JSON.stringify({ result: 'PASS', release, node: process.version, browser: browser.version(), binaryHashes: Object.fromEntries(['taskd', 'taskctl'].map((name) => [name, sha(fs.readFileSync(binary(name)))])), artifacts, checks: ['no business controls/POST', 'desktop/390px layout', 'strict CSP', localAuth ? 'local automatic authorization/logout' : 'reader login/logout', 'refresh restores authorization without login replay', 'Checkpoint/notes/Session/history', 'fresh context copy', 'project lookup/filter', 'SSE 503 reconnect and search protection', 'UI update preserves input/cancel', 'profile/provenance in Project and Task context', 'Task/Project/Profile/History unchanged'] }, null, 2));
  succeeded = true;
} finally {
  try { if (browser) await browser.close(); }
  finally {
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      fs.writeFileSync(stop, 'stop');
      let timer;
      await Promise.race([exited, new Promise((resolve) => { timer = setTimeout(resolve, 10_000); })]);
      clearTimeout(timer);
      if (child.exitCode === null && child.signalCode === null) throw new Error(`Isolated daemon did not exit gracefully; retained at ${temp}. No force kill used.`);
    }
    if (succeeded) fs.rmSync(temp, { recursive: true, force: true });
    else console.error('Failed sandbox retained:', temp);
  }
}
