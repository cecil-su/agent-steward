import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFile, spawnSync } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { CONTEXT_MARKER, readTaskContext, registerTaskContext } from '../pi/task-context.mjs';
const execute = promisify(execFile);
if (process.env.STEWARD_REQUIRE_REAL_PI === '1' && !process.env.STEWARD_TEST_PI_BIN) {
  throw new Error('Mandatory real Pi tests require STEWARD_TEST_PI_BIN; refusing silent skip');
}
const cli = resolve(process.env.CARGO_TARGET_DIR || 'target', 'debug', process.platform === 'win32' ? 'taskctl.exe' : 'taskctl');
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'steward-context-entry-'));
  const database = join(root, 'db');
  const run = (...args) => {
    const out = spawnSync(cli, ['--database', database, '--json', ...args], { encoding: 'utf8' });
    assert.equal(out.status, 0, out.stdout + out.stderr); return JSON.parse(out.stdout).data;
  };
  run('project', 'create', '--name', 'One'); run('project', 'create', '--name', 'Two');
  run('task', 'create', 'A', '--project', '1'); run('task', 'create', 'B', '--project', '1');
  run('task', 'create', 'Other', '--project', '2'); run('task', 'create', 'GlobalOnly');
  run('task', 'note', '1', '--if-version', '1', '--type', 'progress', '--text', 'synthetic long-term feedback A');
  function put(name, projectId, status = 'active', id, revision = 1) {
    const payload = { scope: projectId ? 'project' : 'global', projectId, status, contentVersion: 1, content: { name, body: `${name}\nFULL_BODY_END`, sources: [{ kind: 'explicit', evidence: 'feedback A evidence', taskId: 1, taskVersion: 1 }] } };
    const file = join(root, 'rule.json'); writeFileSync(file, JSON.stringify(payload));
    return run('rule', ...(id ? ['update', String(id), '--if-revision', String(revision)] : ['create']), '--reason', 'synthetic feedback', '--input', file);
  }
  put('global-v1', null); put('project-one', 1); put('candidate-hidden', null, 'candidate'); put('disabled-hidden', null, 'disabled'); put('project-two', 2);
  return { root, database, run, put, config: { cli, database, task: '2' }, close: () => rmSync(root, { recursive: true, force: true }) };
}
test('input gate refreshes context, rejects old session/cwd and blocks errors instead of swallowing them', async () => {
  const f = fixture();
  try {
    const handlers = new Map(), notices = [];
    const flags = { 'steward-context-task': '2', 'steward-context-cli': cli, 'steward-context-database': f.database };
    const pi = { registerFlag() {}, getFlag: n => flags[n], on: (n, h) => handlers.set(n, h) };
    let session = 'new-pi-B';
    const ctx = { cwd: f.root, sessionManager: { getSessionId: () => session, getEntries: () => [] }, hasUI: true, ui: { notify: text => notices.push(text) } };
    registerTaskContext(pi);
    handlers.get('session_start')({ reason: 'startup' }, ctx);
    const input = () => handlers.get('input')({ source: 'interactive', text: 'do not mutate anything' }, ctx);
    const original = { role: 'user', content: [{ type: 'text', text: '/skill:reviewprobe user-argument' }], timestamp: 1 };
    const inject = () => handlers.get('context')({ messages: [original] }, ctx);
    assert.deepEqual(await input(), { action: 'continue' });
    let delivered = await inject();
    assert.equal(delivered.messages[0], original);
    assert.match(delivered.messages[1].content[0].text, /global-v1/);
    f.put('global-v2', null, 'active', 1);
    assert.deepEqual(await input(), { action: 'continue' });
    delivered = await inject();
    assert.match(delivered.messages[1].content[0].text, /global-v2/);
    const pendingInput = input();
    session = 'replacement-during-IO'; handlers.get('session_start')({ reason: 'startup' }, ctx);
    assert.equal((await pendingInput).action, 'handled');
    session = 'unrelated-session'; assert.equal((await input()).action, 'handled');
    handlers.get('session_start')({ reason: 'resume' }, ctx); assert.equal((await input()).action, 'handled');
    handlers.get('session_start')({ reason: 'startup' }, ctx); ctx.cwd = 'another'; assert.equal((await input()).action, 'handled');
    ctx.cwd = f.root; flags['steward-context-database'] = join(f.root, 'absent'); assert.equal((await input()).action, 'handled');
    assert.ok(notices.every(n => n.includes('UNAVAILABLE')));
    assert.equal(f.run('task', 'show', '2').task.version, 1); assert.deepEqual(f.run('session', 'list').sessions, []);
  } finally { f.close(); }
});

test('notification exceptions cannot turn a blocked input into a model request', async t => {
  const handlers = new Map();
  registerTaskContext({ registerFlag() {}, getFlag: () => '1', on: (name, handler) => handlers.set(name, handler) });
  t.mock.method(console, 'error', () => {});
  const ctx = { hasUI: true, ui: { notify() { throw new Error('synthetic notification failure'); } } };
  assert.deepEqual(await handlers.get('input')({ text: '/skill:reviewprobe' }, ctx), { action: 'handled' });
});

test('readTaskContext never initializes zero-byte, SQLite schema0, invalid or missing files', async () => {
  const f = fixture();
  try {
    for (const kind of ['zero-byte', 'sqlite-empty', 'schema0-data', 'invalid']) {
      const database = join(f.root, kind);
      if (kind === 'zero-byte' || kind === 'invalid') writeFileSync(database, kind === 'invalid' ? 'invalid database' : '');
      else {
        const py = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-c', 'import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute("VACUUM");\nif sys.argv[2]=="schema0-data": c.execute("CREATE TABLE untouched(value TEXT)"); c.execute("INSERT INTO untouched VALUES(\'keep\')"); c.commit()\nc.close()', database, kind]);
        assert.equal(py.status, 0, py.stderr?.toString());
      }
      const bytes = readFileSync(database), entries = readdirSync(f.root).sort();
      await assert.rejects(readTaskContext({ ...f.config, database }), /UNAVAILABLE/);
      assert.deepEqual(readFileSync(database), bytes);
      assert.deepEqual(readdirSync(f.root).sort(), entries);
    }
    const entries = readdirSync(f.root).sort();
    await assert.rejects(readTaskContext({ ...f.config, database: join(f.root, 'missing-parent', 'db') }), /UNAVAILABLE/);
    assert.deepEqual(readdirSync(f.root).sort(), entries);
    const before = readFileSync(f.database);
    assert.equal((await readTaskContext(f.config)).data.task.id, 2);
    await assert.rejects(readTaskContext({ ...f.config, task: '999' }), /UNAVAILABLE/);
    assert.deepEqual(readFileSync(f.database), before);
  } finally { f.close(); }
});

// Uses the real Pi CLI and agent loop, with a local synthetic provider capturing the actual outgoing model request.
// No credentials, global extensions, session files, business APIs, or code-writing agent tools.
test('fresh real Pi sessions deliver A feedback to B model context through the candidate startup entry', { skip: !process.env.STEWARD_TEST_PI_BIN, timeout: 120000 }, async () => {
  const f = fixture(); const requests = []; const sessionIds = new Set();
  const server = createServer(async (req, res) => {
    let body = ''; for await (const chunk of req) body += chunk;
    requests.push(JSON.parse(body));
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.end(`data: ${JSON.stringify({ id: 'synthetic', object: 'chat.completion.chunk', model: 'capture', choices: [{ index: 0, delta: { role: 'assistant', content: 'SYNTHETIC_OK' }, finish_reason: null }] })}\n\ndata: ${JSON.stringify({ id: 'synthetic', object: 'chat.completion.chunk', model: 'capture', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\ndata: [DONE]\n\n`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const agent = join(f.root, 'agent'); const project = join(f.root, 'project'); mkdirSync(agent); mkdirSync(project);
    writeFileSync(join(project, 'AGENTS.md'), 'Synthetic repository instructions remain independent. No tools or task mutations.');
    writeFileSync(join(agent, 'models.json'), JSON.stringify({ providers: { 'synthetic-steward': { baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api: 'openai-completions', apiKey: 'synthetic-local-only', models: [{ id: 'capture', contextWindow: 200000, maxTokens: 100 }] } } }));
    const env = Object.fromEntries(['PATH', 'Path', 'SystemRoot', 'SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP'].filter(k => process.env[k]).map(k => [k, process.env[k]]));
    Object.assign(env, { HOME: f.root, USERPROFILE: f.root, APPDATA: f.root, LOCALAPPDATA: f.root, PI_CODING_AGENT_DIR: agent, PI_OFFLINE: '1', PI_TELEMETRY: '0' });
    const piBin = resolve(process.env.STEWARD_TEST_PI_BIN);
    assert.equal((await execute(piBin, ['--version'], { env, cwd: project })).stdout.trim(), '0.85.1');
    const probe = join(f.root, 'identity.ts');
    writeFileSync(probe, 'export default function(pi) { pi.on("session_start", (_e, ctx) => console.log("SESSION_ID=" + ctx.sessionManager.getSessionId())); }');
    const entry = fileURLToPath(new URL('../pi/task-context.ts', import.meta.url));
    mkdirSync(join(agent, 'skills', 'reviewprobe'), { recursive: true });
    mkdirSync(join(agent, 'prompts'));
    writeFileSync(join(agent, 'settings.json'), JSON.stringify({ enableSkillCommands: true }));
    writeFileSync(join(agent, 'skills', 'reviewprobe', 'SKILL.md'), '---\nname: reviewprobe\ndescription: Synthetic expansion probe\n---\nSKILL_BODY_SENTINEL. Only acknowledge; never use tools.');
    writeFileSync(join(agent, 'prompts', 'reviewtemplate.md'), '---\ndescription: Synthetic template probe\n---\nTEMPLATE_BODY_SENTINEL first=$1 second=$2 all=$@');
    const before = [f.run('task', 'show', '2'), f.run('history', '2'), f.run('session', 'list'), f.run('project', 'list')];
    const invalidatingEntry = join(f.root, 'identity-change-probe.ts');
    const implementation = fileURLToPath(new URL('../pi/task-context.mjs', import.meta.url));
    writeFileSync(invalidatingEntry, `import {registerTaskContext} from ${JSON.stringify(implementation)};\nexport default pi => registerTaskContext(new Proxy(pi, {get(target, key) { if (key === 'on') return (event, handler) => pi.on(event, (value, ctx) => handler(value, event === 'context' ? {...ctx, cwd: ctx.cwd + '/changed'} : ctx)); return Reflect.get(target, key); }}));`);
    let lastUserText;
    async function start(task, database = f.database, prompt = 'Only acknowledge this synthetic context. Do not use tools.', gate = true, entryPath = entry) {
      const count = requests.length;
      const pending = execute(piBin, ['--offline', '--no-session', '--no-extensions', '-e', entryPath, '-e', probe, '--no-themes', '--no-tools', '--no-approve', '--provider', 'synthetic-steward', '--model', 'capture', ...(gate ? ['--steward-context-cli', cli, '--steward-context-database', database, '--steward-context-task', task] : []), '-p', prompt], { cwd: project, env, timeout: 20000, maxBuffer: 1024 * 1024 });
      pending.child.stdin.end(); // Pi waits for non-TTY stdin EOF before processing its initial prompt.
      const result = await pending.catch(error => { if (entryPath === invalidatingEntry && error.killed !== true && error.signal == null) return error; throw error; });
      const id = result.stdout.split(/\r?\n/).find(s => s.startsWith('SESSION_ID=')); assert.ok(id, result.stdout + result.stderr); assert.ok(!sessionIds.has(id)); sessionIds.add(id);
      if (requests.length === count) { assert.match(result.stderr, /STEWARD_CONTEXT_UNAVAILABLE/); return null; }
      assert.equal(requests.length, count + 1);
      const users = requests.at(-1).messages.filter(m => m.role === 'user').map(m => typeof m.content === 'string' ? m.content : m.content.map(c => c.text || '').join(''));
      lastUserText = users.filter(t => !t.startsWith(CONTEXT_MARKER)).join('\n');
      if (!gate) { assert.ok(!users.some(t => t.startsWith(CONTEXT_MARKER))); return lastUserText; }
      const text = users.find(t => t.startsWith(CONTEXT_MARKER));
      assert.ok(text, JSON.stringify({ users, stdout: result.stdout, stderr: result.stderr }));
      const context = JSON.parse(text.slice(CONTEXT_MARKER.length).split('\n')[0]);
      assert.deepEqual(context, await readTaskContext({ cli, database, task }));
      return context.data.sessionRules.rules;
    }
    assert.deepEqual((await start('2')).map(r => r.content.name), ['global-v1', 'project-one']);
    assert.equal(lastUserText, 'Only acknowledge this synthetic context. Do not use tools.');
    for (const [prompt, sentinel] of [['/skill:reviewprobe alpha', 'SKILL_BODY_SENTINEL'], ['/reviewtemplate alpha "two words"', 'TEMPLATE_BODY_SENTINEL first=alpha second=two words all=alpha two words']]) {
      const baseline = await start('2', f.database, prompt, false);
      assert.ok(baseline.includes(sentinel));
      assert.deepEqual((await start('2', f.database, prompt)).map(r => r.content.name), ['global-v1', 'project-one']);
      assert.equal(lastUserText, baseline, 'Gate must not change skill/template expansion or argument semantics');
    }
    assert.equal(await start('2', f.database, undefined, true, invalidatingEntry), null, 'A late identity mismatch must abort the real agent before any provider request');
    f.put('global-v2', null, 'active', 1); f.put('project-one-v2', 1, 'active', 2);
    const next = await start('2'); assert.deepEqual(next.map(r => r.revision), [2, 2]); assert.equal(next[0].content.sources[0].taskVersion, 1); assert.match(next[0].content.body, /FULL_BODY_END$/);
    assert.deepEqual((await start('3')).map(r => r.content.name), ['global-v2', 'project-two']);
    assert.deepEqual((await start('4')).map(r => r.content.name), ['global-v2']);
    f.run('rule', 'disable', '1', '--if-revision', '2', '--reason', 'withdraw synthetic');
    assert.deepEqual(await start('4'), []);
    const python = process.platform === 'win32' ? 'python' : 'python3';
    const mutateFormat = version => {
      const out = spawnSync(python, ['-c', 'import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute("UPDATE rules SET content_version=? WHERE id=2", (int(sys.argv[2]),)); c.commit(); c.close()', f.database, String(version)]);
      assert.equal(out.status, 0);
    };
    mutateFormat(2); // Storage accepts the future format; today's CLI/gate must block the request.
    assert.equal(await start('2'), null);
    mutateFormat(1);
    assert.equal(await start('999'), null);
    assert.equal(await start('2', join(f.root, 'missing.db')), null);
    assert.equal(await start('2', join(f.root, 'missing.db'), '/skill:reviewprobe alpha'), null);
    assert.equal(await start('2', join(f.root, 'missing.db'), '/reviewtemplate alpha'), null);
    const old = join(f.root, 'schema6.db');
    // Synthetic incompatible source only. The ordinary candidate CLI must refuse it.
    const py = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-c', 'import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute("PRAGMA user_version=6"); c.close()', old]); assert.equal(py.status, 0);
    const oldBytes = readFileSync(old); assert.equal(await start('2', old), null); assert.deepEqual(readFileSync(old), oldBytes);
    assert.deepEqual([f.run('task', 'show', '2'), f.run('history', '2'), f.run('session', 'list'), f.run('project', 'list')], before);
  } finally { await new Promise(resolve => server.close(resolve)); f.close(); }
});
