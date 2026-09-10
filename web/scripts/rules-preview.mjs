// Reuse the user's ONLY manual-preview entry: .local/local-preview.
// Never stop an existing process, touch the formal backend, or create a second user-facing preview.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, writeFileSync, readFileSync, copyFileSync, cpSync, renameSync, openSync, closeSync } from 'node:fs';
import { join, isAbsolute, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
async function main() {
const binaries = process.argv[2];
if (!binaries || !isAbsolute(binaries) || process.argv.length !== 3) throw new Error('Usage: node web/scripts/rules-preview.mjs ABSOLUTE_CANDIDATE_BINARY_DIRECTORY');
const repository = fileURLToPath(new URL('../../', import.meta.url));
const root = join(repository, '.local', 'local-preview');
const candidate = join(root, 'candidate');
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const json = file => JSON.parse(readFileSync(file, 'utf8'));
function plain(file, directory = false) {
  const s = lstatSync(file);
  if (s.isSymbolicLink() || !(directory ? s.isDirectory() : s.isFile())) throw new Error(`Refusing non-plain preview path: ${file}`);
}
function info(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return null;
  if (process.platform === 'win32') {
    const result = spawnSync('pwsh', ['-NoProfile', '-Command', `Get-CimInstance Win32_Process -Filter "ProcessId=${pid}" | Select-Object ProcessId,ExecutablePath,CommandLine,CreationDate | ConvertTo-Json -Compress`], { encoding: 'utf8' });
    if (result.status !== 0) throw new Error('Cannot verify preview process identity');
    return result.stdout.trim() ? JSON.parse(result.stdout) : null;
  }
  try { process.kill(pid, 0); } catch (e) { if (e.code === 'ESRCH') return null; throw e; }
  return { CommandLine: readFileSync(`/proc/${pid}/cmdline`, 'utf8').replaceAll('\0', ' ') };
}
function owns(processInfo, paths) {
  const command = (processInfo?.CommandLine || '').toLowerCase();
  return paths.every(p => command.includes(normalize(p).toLowerCase()));
}
async function status(url) {
  try { const r = await fetch(`${url}/ui/status`, { signal: AbortSignal.timeout(3000) }); return r.ok ? await r.json() : null; } catch { return null; }
}
plain(root, true); plain(join(root, 'ui'), true);
for (const file of ['server.cjs', 'config.json', 'ui/index.html', 'ui/app.js', 'ui/style.css']) plain(join(root, file));
const config = json(join(root, 'config.json'));
const statePath = join(root, 'state.json');
if (existsSync(statePath)) plain(statePath);
for (const file of ['stdout.log', 'stderr.log']) if (existsSync(join(root, file))) plain(join(root, file));
const state = existsSync(statePath) ? json(statePath) : null;
const live = state && info(state.pid);
if (live) {
  if (!owns(live, [join(root, 'server.cjs')])) throw new Error('Recorded preview PID belongs to another process; no changes made');
  const current = await status(state.url);
  if (current?.preview && current.apiContract === 4 && current.dataSource === 'synthetic' && config.apiContract === 4) {
    const managed = json(join(candidate, 'state.json'));
    const liveBackend = info(managed.pid);
    if (!owns(liveBackend, [managed.executable, join(candidate, 'synthetic.db'), join(candidate, 'stop')]) || config.upstream !== managed.url) throw new Error('Existing backend identity/configuration requires inspection');
    const source = join(binaries, process.platform === 'win32' ? 'taskd.exe' : 'taskd');
    if (hash(source) !== hash(managed.executable) || hash(join(root, 'server.cjs')) !== hash(join(repository, 'web', 'scripts', 'local-preview-server.cjs'))
      || ['index.html', 'app.js', 'style.css'].some(f => hash(join(root, 'ui', f)) !== hash(join(repository, 'web', 'dist', f)))) throw new Error('Candidate artifacts changed; gracefully stop verified preview processes before refresh');
    const response = await fetch(`${state.url}/api/tasks/1/context`, { headers: { 'X-Steward-UI-Contract': '4' }, signal: AbortSignal.timeout(5000) });
    const context = await response.json();
    if (!response.ok || !context.ok || context.data.sessionRules?.formatVersion !== 1) throw new Error('Existing preview context unavailable');
    console.log(JSON.stringify({ reused: true, url: state.url, dataSource: 'synthetic', directory: root, stopFile: join(root, 'stop'), backend: managed }, null, 2));
    return;
  }
  throw new Error('Existing preview process must first be identity-verified and gracefully stopped via its own marker; no automatic reload');
}
if (await status(`http://${config.bind}:${config.port}`)) throw new Error('Configured preview address is occupied without matching process identity; no changes made');
const suffix = process.platform === 'win32' ? '.exe' : '';
const sourceDaemon = join(binaries, `taskd${suffix}`), cli = join(binaries, `taskctl${suffix}`);
plain(sourceDaemon); plain(cli);
if (existsSync(candidate)) plain(candidate, true);
else mkdirSync(candidate);
for (const file of ['state.json', 'fixture.json']) if (existsSync(join(candidate, file))) plain(join(candidate, file));
if (existsSync(join(root, 'backups'))) plain(join(root, 'backups'), true);
const database = join(candidate, 'synthetic.db'), backendStop = join(candidate, 'stop');
const backendStatePath = join(candidate, 'state.json'), backendBinary = join(candidate, `taskd${suffix}`);
const backendState = existsSync(backendStatePath) ? json(backendStatePath) : null;
const backendProcess = backendState && info(backendState.pid);
if (backendProcess && !owns(backendProcess, [backendBinary, database, backendStop])) throw new Error('Backend process identity mismatch; no changes made');
if (backendProcess && (!existsSync(backendBinary) || hash(backendBinary) !== hash(sourceDaemon))) throw new Error('Candidate binary changed; gracefully stop the verified isolated backend before updating it');
// Restore evidence is kept within the established preview directory, not in another preview system.
const backup = join(root, 'backups', new Date().toISOString().replaceAll(':', '-'));
mkdirSync(backup, { recursive: true });
for (const file of ['config.json', 'server.cjs', 'state.json']) if (existsSync(join(root, file))) copyFileSync(join(root, file), join(backup, file));
cpSync(join(root, 'ui'), join(backup, 'ui'), { recursive: true });
if (existsSync(backendStatePath)) copyFileSync(backendStatePath, join(backup, 'backend-state.json'));
function moveStop(file, name) { if (existsSync(file)) { plain(file); renameSync(file, join(backup, name)); } }
function task(...args) {
  const r = spawnSync(cli, ['--database', database, '--json', ...args], { encoding: 'utf8' });
  if (r.status !== 0) throw new Error('Isolated candidate CLI failed; no installed-binary fallback');
  return JSON.parse(r.stdout).data;
}
let backendURL = backendState?.url;
let startedBackend = false;
if (!backendProcess) {
  if (existsSync(backendBinary)) { plain(backendBinary); copyFileSync(backendBinary, join(backup, `previous-taskd${suffix}`)); }
  copyFileSync(sourceDaemon, backendBinary);
  const probe = spawnSync(backendBinary, ['--check-database-schema', '--database', database], { encoding: 'utf8' });
  if (probe.status !== 0 || probe.stdout.trim() !== 'databaseSchema=7') throw new Error('Requires compatible Schema7 candidate; no migration is performed');
  if (!existsSync(database)) {
    task('project', 'create', '--name', '合成规则预览项目');
    const file = join(candidate, 'fixture.json');
    writeFileSync(file, JSON.stringify({ title: '0910｜功能｜合成规则接入预览', goal: '查看有效规则与来源', scope: '合成数据，非正式用户偏好', acceptanceCriteria: '人工核对任务、项目概览和上下文复制' }));
    task('task', 'create', '--project', '1', '--input', file);
    for (const [name, projectId, ruleStatus] of [['合成通用规则', null, 'active'], ['合成项目规则', 1, 'active'], ['候选不应带入', null, 'candidate'], ['停用不应带入', null, 'disabled']]) {
      writeFileSync(file, JSON.stringify({ scope: projectId ? 'project' : 'global', projectId, status: ruleStatus, contentVersion: 1, content: { name, body: `${name}正文\n仅合成预览，不代表真实偏好。`, sources: [{ kind: 'explicit', evidence: '合成任务反馈', taskId: 1, taskVersion: 1 }] } }));
      task('rule', 'create', '--reason', '合成预览', '--input', file);
    }
  }
  moveStop(backendStop, 'previous-backend-stop');
  const logKey = Date.now();
  const stdout = join(candidate, `stdout-${logKey}.log`), stderr = join(candidate, `stderr-${logKey}.log`);
  const out = openSync(stdout, 'wx'), err = openSync(stderr, 'wx');
  const child = spawn(backendBinary, ['--no-open', '--bind', '127.0.0.1', '--port', '0', '--database', database, '--runtime-dir', join(candidate, 'runtime'), '--shutdown-file', backendStop], { detached: true, windowsHide: true, stdio: ['ignore', out, err] });
  closeSync(out); closeSync(err); child.unref(); startedBackend = true;
  for (let i = 0; i < 100; i++) {
    backendURL = readFileSync(stdout, 'utf8').match(/Agent Steward: (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if (backendURL) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  writeFileSync(backendStatePath, JSON.stringify({ pid: child.pid, url: backendURL, executable: backendBinary, database, shutdownFile: backendStop, stdout, stderr, dataSource: 'synthetic' }, null, 2));
}
try {
  if (!backendURL || (await status(backendURL))?.apiContract !== 4) throw new Error('Isolated backend API contract unavailable/incompatible');
  const context = task('task', 'context', '1', '--require-read-only');
  if (context.sessionRules?.formatVersion !== 1) throw new Error('Missing sessionRules capability');
  for (const name of ['index.html', 'app.js', 'style.css']) {
    const source = join(repository, 'web', 'dist', name); plain(source);
    const before = hash(source); copyFileSync(source, join(root, 'ui', name));
    if (hash(source) !== before || hash(join(root, 'ui', name)) !== before) throw new Error('UI changed during copy');
  }
  copyFileSync(join(repository, 'web', 'scripts', 'local-preview-server.cjs'), join(root, 'server.cjs'));
  writeFileSync(join(root, 'config.json'), JSON.stringify({ ...config, upstream: backendURL, apiContract: 4, uiVersion: 'rules-candidate', dataSource: 'synthetic' }, null, 2));
  moveStop(join(root, 'stop'), 'previous-preview-stop');
  const out = openSync(join(root, 'stdout.log'), 'a'), err = openSync(join(root, 'stderr.log'), 'a');
  const front = spawn(process.execPath, [join(root, 'server.cjs')], { cwd: root, detached: true, windowsHide: true, stdio: ['ignore', out, err] });
  closeSync(out); closeSync(err); front.unref();
  for (let i = 0; i < 100; i++) {
    if (existsSync(statePath)) {
      const fresh = json(statePath);
      if (fresh.pid === front.pid && (await status(fresh.url))?.apiContract === 4) {
        console.log(JSON.stringify({ reused: false, url: fresh.url, dataSource: 'synthetic', directory: root, backup, frontend: { pid: front.pid, stopFile: fresh.stopFile, purpose: 'sole user-facing local GET-only proxy and UI' }, backend: { ...json(backendStatePath), purpose: 'isolated Schema7 data, loopback only; not a separate user preview entry' }, humanAcceptance: 'not confirmed' }, null, 2));
        return;
      }
    }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  writeFileSync(join(root, 'stop'), 'stop'); // Only the process just started by this script watches this marker.
  throw new Error('Preview failed to become ready; backup retained for explicit restoration');
} catch (error) {
  if (startedBackend) writeFileSync(backendStop, 'stop');
  throw error;
}
}
await main();
