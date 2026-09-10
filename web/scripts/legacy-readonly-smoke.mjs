// Native readonly candidate only. Never uses an installed URL or the default database.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const source = path.join(root, 'crates/server/web-legacy-readonly');
const binDir = process.env.STEWARD_TEST_BIN_DIR;
const bind = process.env.STEWARD_TEST_BIND ?? '127.0.0.1';
assert(Object.values(os.networkInterfaces()).flat().some(entry => entry?.family === 'IPv4' && entry.address === bind), 'Bind must be a local IPv4 address');
assert(binDir && path.isAbsolute(binDir), 'Set STEWARD_TEST_BIN_DIR to an absolute development binary directory');
const binary = name => path.join(binDir, name + (process.platform === 'win32' ? '.exe' : ''));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'steward-native-detail-'));
const database = path.join(temp, 'synthetic.db'), stop = path.join(temp, 'stop');
const artifacts = path.join(root, 'web/.artifacts', 'native-' + new Date().toISOString().replaceAll(':', '-'));
fs.mkdirSync(artifacts, { recursive: true });
const cli = (...args) => {
  const response = JSON.parse(execFileSync(binary('taskctl'), ['--database', database, '--json', ...args], { encoding: 'utf8' }));
  assert(response.ok, 'Synthetic CLI command failed'); return response.data;
};
const input = (name, value) => { const file = path.join(temp, name + '.json'); fs.writeFileSync(file, JSON.stringify(value)); return file; };
const snapshot = () => execFileSync('python', ['-c', `import sqlite3,hashlib,pathlib,sys
c=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True)
c.execute('BEGIN')
h=hashlib.sha256()
for (name,) in c.execute("select name from sqlite_master where type='table' order by name"):
 h.update(name.encode())
 for row in c.execute('select * from "'+name.replace('"','""')+'" order by rowid'):
  h.update(repr(row).encode())
print(h.hexdigest())`, database], { encoding: 'utf8' }).trim();
let child, browser, exited = Promise.resolve(), success = false;
try {
  const p = cli('project', 'create', '--name', '原生隔离项目').project;
  const empty = cli('project', 'create', '--name', '空资料项目').project;
  const noTasks = cli('project', 'create', '--name', '无任务项目').project;
  const createTask = (name, project = p.id) => cli('task', 'create', '--input', input('task', { title: `0909｜功能｜${name}`, goal: '任务主体目标', scope: '任务主体范围', acceptanceCriteria: '任务主体验收', ...(project ? { project: String(project) } : {}) })).task;
  const origin = createTask('已关闭来源任务');
  const profile = { summary: '简介长文本\n' + '原生展示LongText'.repeat(170), architecture: '架构入口独占项目页\n<script>不可执行</script>', development: '开发验证独占项目页\ncargo test', evidence: '合成核实依据，不是正式事实', sourceTaskId: origin.id, sourceTaskVersion: origin.version };
  let revision = p.revision;
  const setProfile = value => { revision = cli('project', 'profile', 'set', String(p.id), '--if-revision', String(revision), '--input', input('profile', value)).project.revision; };
  setProfile({ ...profile, summary: '修改前简介' }); setProfile(profile);
  const component = cli('project', 'component', 'add', String(p.id), '--name', 'frontend', '--if-revision', String(revision)).component;
  revision++;
  const directory = path.join(temp, 'registered-directory'); fs.mkdirSync(directory);
  fs.writeFileSync(path.join(directory, 'README.md'), 'Synthetic navigation only');
  cli('project', 'source', 'add', String(p.id), '--directory', directory, '--if-revision', String(revision)); revision++;
  const repo = path.join(temp, 'registered-repo'); fs.mkdirSync(repo);
  execFileSync('git', ['-C', repo, 'init', '-b', 'main'], { stdio: 'pipe' });
  fs.writeFileSync(path.join(repo, 'README.md'), 'Synthetic repository navigation');
  cli('project', 'source', 'add', String(p.id), '--repo', repo, '--path', '.', '--component', 'frontend', '--if-revision', String(revision)); revision++;
  // More than one real API page for both tasks and project history.
  for (let i = 0; i < 46; i++) { cli('project', 'rename', String(p.id), '--name', i % 2 ? p.name : '历史临时名称', '--if-revision', String(revision)); revision++; }
  cli('task', 'close', String(origin.id), '--if-version', String(origin.version), '--outcome', 'cancelled', '--reason', '隔离已关闭导航用例', '--yes');
  for (let i = 0; i < 30; i++) createTask('分页任务' + i);
  const unbounded = createTask('未限定组件任务');
  const scoped = createTask('指定组件任务');
  cli('task', 'components', String(scoped.id), '--component', 'frontend', '--if-version', String(scoped.version), '--yes', '--reason', 'Synthetic component scope fixture');
  const noProject = createTask('无项目任务', null);
  const emptyTask = createTask('资料未填写任务', empty.id);
  cli('task', 'claim', String(noProject.id), '--session', 'synthetic-pending', '--if-version', String(noProject.version));
  cli('task', 'pending-release', String(noProject.id), '--if-version', String(noProject.version + 1));
  const ruleBody = '规则完整正文\n<img src=x onerror=alert(1)>';
  for (const [name, projectId, ruleStatus] of [['合成通用规则', null, 'active'], ['合成项目规则', p.id, 'active'], ['candidate-hidden', null, 'candidate'], ['disabled-hidden', null, 'disabled']]) {
    cli('rule', 'create', '--reason', 'Synthetic native rules fixture', '--input', input('rule', { scope: projectId ? 'project' : 'global', projectId, status: ruleStatus, contentVersion: 1, content: { name, body: ruleBody, sources: [{ kind: 'explicit', evidence: '历史版本依据', taskId: origin.id, taskVersion: origin.version }] } }));
  }
  const baseline = snapshot();
  const resources = Object.fromEntries(['index.html', 'app.js', 'style.css'].map(name => [name, fs.readFileSync(path.join(source, name))]));
  const manifest = Buffer.from(JSON.stringify({ packageFormat: 1, uiVersion: 'native-task45-test', requiredApiContract: 4, entry: 'index.html', files: Object.fromEntries(Object.entries(resources).map(([name, bytes]) => [name, sha(bytes)])) }));
  const release = sha(manifest), uiRoot = path.join(temp, 'ui'), releaseDir = path.join(uiRoot, 'releases', release);
  fs.mkdirSync(releaseDir, { recursive: true });
  fs.writeFileSync(path.join(releaseDir, 'manifest.json'), manifest);
  for (const [name, bytes] of Object.entries(resources)) fs.writeFileSync(path.join(releaseDir, name), bytes);
  fs.writeFileSync(path.join(uiRoot, 'current.json'), JSON.stringify({ release }));
  child = spawn(binary('taskd'), ['--database', database, '--runtime-dir', path.join(temp, 'runtime'), '--ui-root', uiRoot, '--shutdown-file', stop, '--require-local-auth', '--bind', bind, '--port', '0', '--no-open'], { stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', startupError;
  child.stdout.on('data', bytes => { stdout += bytes; }); child.stderr.resume();
  exited = new Promise(resolve => { child.once('exit', resolve); child.once('error', error => { startupError = error; resolve(); }); });
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  for (let i = 0; i < 200 && !stdout.includes('Credential file:'); i++) { if (startupError) throw startupError; assert.equal(child.exitCode, null); await delay(50); }
  const url = stdout.match(/Agent Steward: (http:\/\/[\d.]+:\d+)/)?.[1];
  const credentialPath = stdout.match(/Credential file: (.+)/)?.[1]?.trim();
  assert(url && credentialPath, 'Isolated startup did not advertise URL/credential path');
  const status = await fetch(url + '/ui/status').then(r => r.json()); assert.equal(status.release, release); assert.equal(status.error, null);
  browser = await chromium.launch({ headless: true, channel: process.env.STEWARD_BROWSER_CHANNEL === 'chromium' ? undefined : process.env.STEWARD_BROWSER_CHANNEL ?? 'chrome' });
  const context = await browser.newContext({ viewport: { width: 1360, height: 1000 } });
  const page = await context.newPage(), errors = [], posts = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.addInitScript(() => { window.__csp = []; document.addEventListener('securitypolicyviolation', e => window.__csp.push(e.violatedDirective)); });
  await page.route('**/api/**', async route => {
    const request = route.request(), pathname = new URL(request.url()).pathname;
    if (request.method() !== 'GET') { posts.push(pathname); if (!['/api/login', '/api/logout'].includes(pathname)) { await route.abort(); return; } }
    await route.continue();
  });
  const token = fs.readFileSync(path.join(path.dirname(credentialPath), 'readonly-credential'), 'utf8').trim();
  await page.goto(url);
  try { await page.getByLabel('本次服务的连接凭据').fill(token); await page.getByRole('button', { name: '连接工作台' }).click(); await page.locator('#workspace').waitFor({ state: 'visible' }); }
  catch { await page.screenshot({ path: path.join(artifacts, 'login-failure.png') }); console.error('Login diagnostic:', await page.locator('#login-error').textContent({ timeout: 1000 }).catch(() => 'Application login page unavailable'), errors); throw Error('Synthetic reader login failed (credential omitted)'); }
  for (const number of [String(noProject.id), '#' + noProject.id]) {
    await page.locator('#search').fill(number); await page.locator('#search-form button').click();
    await page.waitForFunction(id => { const cards = document.querySelectorAll('#task-list .task-card'); return cards.length === 1 && cards[0].querySelector('.card-meta span')?.textContent === '#' + id; }, noProject.id);
  }
  await page.locator('#search').fill(''); await page.locator('#search-form button').click();
  await page.waitForFunction(() => document.querySelectorAll('#task-list .task-card').length === 30);
  const task = async name => { await page.locator('#task-list .task-card').filter({ hasText: name }).click(); await page.locator('#detail h2').filter({ hasText: name }).waitFor(); };
  const project = async name => { await page.locator('#projects').click(); await page.locator('#project-list .task-card').filter({ hasText: name }).click(); await page.locator('#project-detail h2').filter({ hasText: name }).waitFor(); };
  const projectRoot = page.locator('#project-detail');
  await task('无项目任务'); await page.getByText('未关联项目', { exact: true }).waitFor();
  await page.locator('#detail .badge.pending_release').waitFor();
  await task('资料未填写任务'); await page.locator('#detail summary').filter({ hasText: '展开项目简介' }).click(); await page.getByText('项目简介尚未填写。', { exact: true }).waitFor();
  await task('未限定组件任务'); await page.getByText('当前组件范围：未限定组件', { exact: true }).waitFor();
  const rulesPanel = page.locator('#detail .session-rules');
  await rulesPanel.getByText('本机通用 · 合成通用规则 · revision 1', { exact: true }).click();
  await rulesPanel.locator('details[open]').getByText(ruleBody, { exact: true }).waitFor();
  assert.equal(await rulesPanel.locator('img,script').count(), 0);
  assert.equal(await rulesPanel.getByText(/candidate-hidden|disabled-hidden/).count(), 0);
  await page.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { window.__copied = text; } } }); });
  await page.getByRole('button', { name: '复制交接上下文', exact: true }).click();
  await page.waitForFunction(() => window.__copied?.includes('规则完整正文'));
  const copied = await page.evaluate(() => window.__copied);
  for (const text of ['历史版本依据', 'sourceTaskVersion', '"taskVersion": 1', '不构成执行授权']) assert(copied.includes(text));
  assert.equal(await page.locator('#detail').getByText('架构入口独占项目页', { exact: false }).count(), 0);
  assert.equal(await page.locator('#detail').getByText(profile.summary, { exact: true }).isVisible(), false);
  await page.locator('#detail summary').filter({ hasText: '展开项目简介' }).click();
  await page.locator('#detail').getByText(profile.summary, { exact: true }).waitFor();
  await page.locator('#refresh').click(); await page.locator('#detail').getByText(profile.summary, { exact: true }).waitFor();
  await page.locator('#detail').screenshot({ path: path.join(artifacts, 'task-desktop.png') });
  await task('指定组件任务'); await page.getByText(`当前组件范围：frontend (#${component.id})`, { exact: true }).waitFor();
  await page.getByRole('tab', { name: '代码现场' }).click(); await page.getByText('未关联 Worktree', { exact: true }).waitFor();
  assert.equal(await page.locator('#detail').getByText(directory, { exact: false }).count(), 0);
  await page.getByRole('tab', { name: '概览' }).click();
  await page.getByRole('button', { name: '查看完整项目资料', exact: true }).click();
  await projectRoot.getByText(profile.summary, { exact: true }).waitFor();
  await page.locator('#project-list .task-card').filter({ hasText: p.name }).waitFor();
  assert.equal(await page.locator('#project-list').getByText('暂无项目', { exact: true }).count(), 0);
  await projectRoot.getByText(profile.architecture, { exact: true }).waitFor();
  await projectRoot.getByText(profile.development, { exact: true }).waitFor();
  await projectRoot.getByText(profile.evidence, { exact: true }).waitFor();
  await projectRoot.locator('p').filter({ hasText: /^普通目录：.*registered-directory$/ }).waitFor();
  await projectRoot.getByText('登记 common-dir：', { exact: false }).waitFor();
  await projectRoot.getByRole('button', { name: '加载更多关联任务', exact: true }).click();
  await projectRoot.getByRole('button', { name: `#${origin.id} · 0909｜功能｜已关闭来源任务`, exact: true }).waitFor();
  await projectRoot.getByRole('button', { name: '加载更多维护历史', exact: true }).click();
  await projectRoot.getByText(`revision ${revision} ·`, { exact: false }).waitFor();
  const changes = projectRoot.locator('.timeline-item').filter({ hasText: 'project.profile_updated' });
  await changes.nth(1).locator('summary').first().click();
  assert((await changes.nth(1).innerText()).includes('修改前简介'));
  assert((await changes.nth(1).innerText()).includes('修改后'));
  assert((await changes.nth(1).innerText()).includes(JSON.stringify(profile.summary).slice(1, -1)));
  await page.screenshot({ path: path.join(artifacts, 'project-desktop.png'), fullPage: true });
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Project mobile overflow');
    await page.screenshot({ path: path.join(artifacts, `project-${width}.png`), fullPage: true });
  }
  await projectRoot.getByRole('button', { name: `#${origin.id} · 0909｜功能｜已关闭来源任务`, exact: true }).click();
  await page.locator('#detail h2').filter({ hasText: '已关闭来源任务' }).waitFor();
  await page.locator('#detail .badge.closed').waitFor();
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Task mobile overflow');
  await page.screenshot({ path: path.join(artifacts, 'task-320.png'), fullPage: true });
  await page.locator('#detail').getByRole('button', { name: `${p.name} (##${p.id})`, exact: true }).click();
  await projectRoot.getByRole('button', { name: `来源任务 #${origin.id}`, exact: true }).first().click();
  await page.locator('#detail .badge.closed').waitFor();
  await project(empty.name); await projectRoot.getByText('项目资料尚未填写。', { exact: true }).waitFor();
  await projectRoot.getByText('尚未登记组件。', { exact: true }).waitFor(); await projectRoot.getByText('尚未登记源码。', { exact: true }).waitFor();
  // Distinguish errors from empty values; retry only reads and late responses cannot replace another selection.
  const fail = route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ ok: false, error: { code: 'SERVER_BUSY', message: '合成读取故障' } }) });
  await page.route(`**/api/projects/${p.id}`, fail); await page.locator('#project-list .task-card').filter({ hasText: p.name }).click();
  await projectRoot.getByText('项目资料读取失败：', { exact: false }).waitFor();
  assert.equal(await projectRoot.getByText('项目资料尚未填写。', { exact: true }).count(), 0);
  await page.unroute(`**/api/projects/${p.id}`, fail);
  await projectRoot.getByRole('button', { name: '重试读取项目资料', exact: true }).click(); await projectRoot.getByText(profile.summary, { exact: true }).waitFor();
  for (const [suffix, label] of [['components', '组件'], ['sources', '源码登记'], ['history?**', '维护历史']]) {
    const pattern = `**/api/projects/${p.id}/${suffix}`;
    await page.route(pattern, fail); await page.locator('#refresh').click();
    await projectRoot.getByText(label + '读取失败：', { exact: false }).waitFor();
    await projectRoot.getByText(profile.summary, { exact: true }).waitFor();
    await page.unroute(pattern, fail); await projectRoot.getByRole('button', { name: '重试读取' + label, exact: true }).click();
    await projectRoot.getByText(label + '读取失败：', { exact: false }).waitFor({ state: 'hidden' });
  }
  const taskPattern = '**/api/tasks?**';
  const failRelated = async route => { if (new URL(route.request().url()).searchParams.get('project') === String(p.id)) await fail(route); else await route.continue(); };
  await page.route(taskPattern, failRelated); await page.locator('#refresh').click(); await projectRoot.getByText('关联任务读取失败：', { exact: false }).waitFor();
  await page.unroute(taskPattern, failRelated); await projectRoot.getByRole('button', { name: '重试读取关联任务', exact: true }).click();
  await projectRoot.getByRole('button', { name: '加载更多关联任务', exact: true }).waitFor();
  let releaseHeld, markHeld;
  const held = new Promise(resolve => { releaseHeld = resolve; }), started = new Promise(resolve => { markHeld = resolve; });
  const slow = async route => { const response = await route.fetch(); markHeld(); await held; await route.fulfill({ response }); };
  await page.route(`**/api/projects/${p.id}`, slow);
  await page.locator('#refresh').click(); await started;
  await page.locator('#project-list .task-card').filter({ hasText: empty.name }).click();
  await projectRoot.getByText('项目资料尚未填写。', { exact: true }).waitFor();
  releaseHeld(); await page.unroute(`**/api/projects/${p.id}`, slow); await delay(300);
  assert.equal(await projectRoot.locator('h2').innerText(), empty.name);
  await project(noTasks.name); await projectRoot.getByText('暂无关联任务。', { exact: true }).waitFor();
  // Older backend field absence is not an unfilled record.
  const withoutProfile = async route => { const response = await route.fetch(); const body = await response.json(); delete body.data.profile; await route.fulfill({ response, json: body }); };
  await page.route(`**/api/projects/${empty.id}`, withoutProfile); await project(empty.name);
  await projectRoot.getByText('后端未提供项目资料字段，无法读取资料。', { exact: true }).waitFor();
  assert.equal(await projectRoot.getByText('项目资料尚未填写。', { exact: true }).count(), 0);
  await page.unroute(`**/api/projects/${empty.id}`, withoutProfile);
  // Failed later pages retain existing entries and retry the same read cursor.
  await project(p.name);
  await projectRoot.getByRole('button', { name: '加载更多关联任务', exact: true }).waitFor();
  const firstCount = await projectRoot.getByRole('button', { name: /^#\d+ ·/ }).count();
  const failLater = async route => { const u = new URL(route.request().url()); if (u.searchParams.has('cursor')) await fail(route); else await route.continue(); };
  await page.route('**/api/tasks?**', failLater); await projectRoot.getByRole('button', { name: '加载更多关联任务', exact: true }).click();
  await projectRoot.getByText('关联任务读取失败：', { exact: false }).waitFor();
  assert.equal(await projectRoot.getByRole('button', { name: /^#\d+ ·/ }).count(), firstCount);
  await page.unroute('**/api/tasks?**', failLater); await projectRoot.getByRole('button', { name: '重试读取关联任务', exact: true }).click();
  await projectRoot.getByRole('button', { name: `#${origin.id} · 0909｜功能｜已关闭来源任务`, exact: true }).waitFor();
  assert.equal(await projectRoot.getByRole('button', { name: /^#\d+ ·/ }).count(), 33);
  // Task component lookup failures retain the selected IDs, never claim an unrestricted scope.
  await page.locator('#back-tasks').click();
  await page.route(`**/api/projects/${p.id}/components`, fail); await task('指定组件任务');
  await page.locator('#detail').getByText('组件名称读取失败：', { exact: false }).waitFor();
  assert((await page.locator('#detail .task-project').innerText()).includes('#' + component.id));
  assert.equal(await page.locator('#detail').getByText('当前组件范围：未限定组件', { exact: true }).count(), 0);
  await page.unroute(`**/api/projects/${p.id}/components`, fail); await page.getByRole('button', { name: '重试读取组件名称', exact: true }).click();
  await page.getByText(`当前组件范围：frontend (#${component.id})`, { exact: true }).waitFor();
  const missingTaskFields = async route => { const response = await route.fetch(); const body = await response.json(); delete body.data.projectProfile; delete body.data.task.componentIds; delete body.data.sessionRules; await route.fulfill({ response, json: body }); };
  await page.route(`**/api/tasks/${scoped.id}/context`, missingTaskFields); await page.locator('#refresh').click();
  await page.getByText('后端未提供组件范围。', { exact: true }).waitFor(); await page.locator('#detail summary').filter({ hasText: '展开项目简介' }).click();
  await page.getByText('后端未提供项目资料字段，无法读取简介。', { exact: true }).waitFor();
  await page.locator('#detail .session-rules .read-error').waitFor();
  await page.evaluate(() => { window.__copied = ''; });
  await page.getByRole('button', { name: '复制交接上下文', exact: true }).click();
  await page.locator('#notice').getByText('有效规则不可用或格式不受支持，请刷新核对；不能按空规则继续。', { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.__copied), '');
  await page.unroute(`**/api/tasks/${scoped.id}/context`, missingTaskFields);
  // Failed context clears the prior task rather than presenting it as the newly selected task.
  await page.route(`**/api/tasks/${unbounded.id}/context`, fail);
  await page.locator('#task-list .task-card').filter({ hasText: '未限定组件任务' }).click();
  await page.locator('#detail').getByText('任务读取失败：', { exact: false }).waitFor(); assert.equal(await page.locator('#detail h2').count(), 0);
  await page.unroute(`**/api/tasks/${unbounded.id}/context`, fail); await page.getByRole('button', { name: '重试读取任务', exact: true }).click();
  await page.locator('#detail h2').filter({ hasText: '未限定组件任务' }).waitFor();
  assert.equal(await page.locator('button:visible').filter({ hasText: /新建任务|新建项目|修改项目名称|记录进展|关闭任务|登记普通目录|添加组件|确认提交/ }).count(), 0);
  assert.deepEqual(await page.evaluate(() => window.__csp), []); assert.deepEqual(errors, []);
  await page.locator('#logout').click(); await page.locator('#login').waitFor({ state: 'visible' });
  // Even an admin connection remains a readonly UI.
  const admin = fs.readFileSync(credentialPath, 'utf8').trim();
  try { await page.getByLabel('本次服务的连接凭据').fill(admin); await page.getByRole('button', { name: '连接工作台' }).click(); await page.locator('#workspace').waitFor({ state: 'visible' }); }
  catch { throw Error('Synthetic admin login failed (credential omitted)'); }
  assert.equal(await page.locator('#create').isVisible(), false); await project(p.name);
  assert.equal(await page.getByRole('button', { name: '修改项目名称', exact: true }).isVisible().catch(() => false), false);
  await page.locator('#logout').click();
  assert.deepEqual(posts, ['/api/login', '/api/logout', '/api/login', '/api/logout']);
  assert.equal(snapshot(), baseline, 'Browser changed synthetic database tables');
  const result = { result: 'PASS', uiVersion: status.uiVersion, release, node: process.version, browser: browser.version(), binaryHashes: Object.fromEntries(['taskd', 'taskctl'].map(name => [name, sha(fs.readFileSync(binary(name)))])), artifacts, checks: ['numeric and hash-prefixed task search', 'contract4 and pending release', 'full rules/source copy and missing-rule copy rejection', 'full profile/provenance', 'compact task project', 'component scope', 'project source versus task Worktree', 'closed task navigation', 'real task/history pagination', 'before/after history', 'empty/missing/error states', 'read retry and stale project response', '1360/390/320px layout and CSP', 'reader/admin readonly boundary', 'all SQLite tables unchanged'] };
  fs.writeFileSync(path.join(artifacts, 'result.json'), JSON.stringify(result, null, 2)); console.log(JSON.stringify(result, null, 2)); success = true;
} finally {
  try { if (browser) await browser.close(); }
  finally {
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      fs.writeFileSync(stop, 'stop'); let timer;
      await Promise.race([exited, new Promise(resolve => { timer = setTimeout(resolve, 10000); })]); clearTimeout(timer);
      if (child.exitCode === null && child.signalCode === null) throw Error('Isolated daemon did not exit gracefully; sandbox retained. No force kill used.');
    }
    if (success) fs.rmSync(temp, { recursive: true }); else console.error('Failed sandbox retained:', temp);
  }
}
