// Isolated adapter contract tests: no taskctl process, Pi installation, or database access.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import test from 'node:test';

let source = await readFile(new URL('../pi/agent-steward.ts', import.meta.url), 'utf8');
source = source.replace('import { StringEnum } from "@earendil-works/pi-ai";', 'const StringEnum = (values) => ({ enum: values });');
source = source.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: (_target, kind) => (...args) => ({ kind, args }) });');
const { registerAgentSteward } = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(source)).toString('base64')}`);
const states = ['backlog', 'todo', 'in_progress', 'in_review', 'blocked', 'done', 'cancelled'];
const task = (overrides = {}) => ({ id: 59, taskKey: null, title: '0521｜功能｜合成测试', status: 'backlog', version: 7, currentSessionId: null, nextStep: null, ...overrides });
const ok = (value = task()) => ({ schemaVersion: 3, ok: true, data: { task: value }, warnings: [], error: null });
const failure = (code) => ({ schemaVersion: 3, ok: false, data: null, warnings: [], error: { code, message: 'fixture', retryable: true, details: {} } });
function harness(responses = []) {
  const calls = [], events = new Map(), commands = new Map();
  let tool;
  registerAgentSteward({ registerTool(value) { tool = value; }, on(name, fn) { events.set(name, fn); }, registerCommand(name, command) { commands.set(name, command); } }, async (command, args, stdin, signal) => {
    calls.push({ command, args, stdin, signal });
    assert.ok(responses.length, 'unexpected taskctl call');
    const envelope = responses.shift();
    return { stdout: JSON.stringify(envelope), stderr: '', code: envelope.ok ? 0 : 4 };
  });
  const ctx = { sessionManager: { getSessionId: () => 'test-pi-session' }, hasUI: true, ui: { notify() {} } };
  return { calls, events, commands, ctx, tool, execute: (params) => tool.execute('test-call', params, undefined, undefined, ctx) };
}
const commandArgs = (call) => call.args.slice(call.args.indexOf('task'));

test('startup and ordinary prompts never discover, claim, attach, or access databases', async () => {
  const h = harness();
  await h.events.get('session_start')({}, h.ctx);
  assert.equal(await h.events.get('before_agent_start')({ systemPrompt: 'base' }), undefined);
  await h.commands.get('steward-status').handler('', h.ctx);
  assert.deepEqual(h.calls, []);
  await assert.rejects(h.execute({ action: 'note', noteType: 'progress', text: 'x' }), /taskId is required/);
});

test('seven-state envelope v3 decoding and show never imply ownership or execution authorization', async () => {
  for (const status of states) {
    const h = harness([ok(task({ status }))]);
    const result = await h.execute({ action: 'show', taskId: 59 });
    assert.match(result.content[0].text, new RegExp(`status=${status}`));
    assert.equal(await h.events.get('before_agent_start')({ systemPrompt: 'base' }), undefined);
    assert.deepEqual(commandArgs(h.calls[0]), ['task', 'show', '59']);
  }
});

test('status sends only selected status with confirmed CAS, independent of sessions', async () => {
  for (const status of states) {
    const h = harness([ok(), ok(task({ status, version: 8 }))]);
    await h.execute({ action: 'status', taskId: 59, status, expectedVersion: 7, confirmedByUser: true });
    assert.deepEqual(commandArgs(h.calls[1]), ['task', 'status', '59', status, '--if-version', '7']);
    assert.equal(h.calls[1].stdin, undefined);
  }
});

test('status/update/retitle/claim require user confirmation before any process call', async () => {
  for (const action of ['status', 'update', 'retitle', 'claim']) {
    const h = harness();
    await assert.rejects(h.execute({ action, taskId: 59, status: 'done', expectedVersion: 7 }), /confirmedByUser/);
    assert.deepEqual(h.calls, []);
  }
  for (const action of ['status', 'update', 'retitle']) {
    const h = harness();
    await assert.rejects(h.execute({ action, taskId: 59, confirmedByUser: true }), /expectedVersion/);
    assert.deepEqual(h.calls, []);
  }
});

test('stale confirmation refreshes only and does not submit or retry a mutation', async () => {
  const h = harness([ok(task({ version: 8 }))]);
  await assert.rejects(h.execute({ action: 'status', taskId: 59, status: 'done', expectedVersion: 7, confirmedByUser: true }), /VERSION_CONFLICT/);
  assert.equal(h.calls.length, 1);
  assert.deepEqual(commandArgs(h.calls[0]), ['task', 'show', '59']);
});

test('server CAS conflict refreshes once and never retries failed mutation', async () => {
  const h = harness([ok(), failure('VERSION_CONFLICT'), ok(task({ version: 9 }))]);
  await assert.rejects(h.execute({ action: 'status', taskId: 59, status: 'done', expectedVersion: 7, confirmedByUser: true }), /not retried/);
  assert.deepEqual(h.calls.map((call) => commandArgs(call)[1]), ['show', 'status', 'show']);
});

test('claim attaches a Session but never sends task status or implicit takeover', async () => {
  const h = harness([ok(task({ status: 'done' })), ok(task({ status: 'done', version: 8, currentSessionId: 'pi-test-pi-session-fixture' }))]);
  await h.execute({ action: 'claim', taskId: 59, confirmedByUser: true });
  const args = commandArgs(h.calls[1]);
  assert.equal(args[1], 'claim');
  assert.match(args[args.indexOf('--session') + 1], /^pi-test-pi-session-/);
  assert.equal(args.includes('--status'), false);
  assert.equal(args.includes('--take-over'), false);
  assert.match((await h.events.get('before_agent_start')({ systemPrompt: 'base' })).systemPrompt, /do not grant execution permission/);
  assert.deepEqual(h.calls.map((call) => commandArgs(call)[1]), ['show', 'claim']);
});

test('update and retitle preserve exact confirmed payload and version', async () => {
  const h = harness([ok(), ok(task({ version: 8 }))]);
  await h.execute({ action: 'update', taskId: 59, goal: 'confirmed goal', nextStep: null, reason: 'explicit patch', expectedVersion: 7, confirmedByUser: true });
  assert.deepEqual(JSON.parse(h.calls[1].stdin), { goal: 'confirmed goal', nextStep: null });
  assert.deepEqual(commandArgs(h.calls[1]), ['task', 'update', '--yes', '59', '--if-version', '7', '--reason', 'explicit patch']);
  const r = harness([ok(), ok(task({ version: 8 }))]);
  await r.execute({ action: 'retitle', taskId: 59, title: '0521｜修复｜合成测试', expectedVersion: 7, confirmedByUser: true });
  assert.deepEqual(commandArgs(r.calls[1]), ['task', 'retitle', '59', '--if-version', '7', '--title', '0521｜修复｜合成测试']);
});

test('retired actions and unsupported envelopes cannot mutate', async () => {
  for (const action of ['close', 'block', 'unblock', 'pending_release']) {
    const h = harness();
    await assert.rejects(h.execute({ action, taskId: 59 }), /Unsupported action/);
    assert.equal(h.calls.length, 0);
  }
  for (const schemaVersion of [2, 4]) {
    const h = harness([{ ...ok(), schemaVersion }]);
    await assert.rejects(h.execute({ action: 'status', taskId: 59, status: 'done', expectedVersion: 7, confirmedByUser: true }), /ADAPTER_UNSUPPORTED_SCHEMA/);
    assert.equal(h.calls.length, 1);
  }
});

test('malformed snapshots and envelopes fail closed before mutations', async () => {
  for (const envelope of [ok(task({ status: 'closed' })), ok(task({ version: 0 })), { schemaVersion: 3, ok: true }]) {
    const h = harness([envelope]);
    await assert.rejects(h.execute({ action: 'status', taskId: 59, status: 'done', expectedVersion: 7, confirmedByUser: true }), /ADAPTER_INVALID_OUTPUT/);
    assert.equal(h.calls.length, 1);
  }
});

test('checkpoint uses owned Session and stdin, without changing status', async () => {
  const owned = task({ currentSessionId: 'pi-test-pi-session-fixture', status: 'cancelled' });
  const h = harness([ok(owned), ok(owned), ok({ ...owned, version: 8 })]);
  await h.execute({ action: 'checkpoint', taskId: 59, summary: 'snapshot', nextStep: 'wait for user' });
  assert.deepEqual(h.calls.map((call) => commandArgs(call)[1]), ['show', 'show', 'checkpoint']);
  assert.equal(JSON.parse(h.calls[2].stdin).summary, 'snapshot');
  assert.equal(commandArgs(h.calls[2]).includes('--status'), false);
});
