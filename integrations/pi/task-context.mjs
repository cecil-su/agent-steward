// Opt-in task input gate. It never discovers/claims tasks or mutates Steward state.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { isAbsolute } from 'node:path';
import { stat } from 'node:fs/promises';
const execute = promisify(execFile);
const positive = n => Number.isSafeInteger(n) && n > 0;
const text = s => typeof s === 'string' && s.trim().length > 0;
export const CONTEXT_MARKER = 'Steward task context (read-only snapshot; not execution authorization):\n';
const unavailable = () => new Error('STEWARD_CONTEXT_UNAVAILABLE: input blocked; verify explicit CLI/database/task and supported context format. No empty-rule fallback.');
function reportBlocked(ctx) {
  // A notification failure must never turn a blocked input into a model request.
  try { console.error(unavailable().message); } catch {}
  try { if (ctx.hasUI) ctx.ui.notify(unavailable().message, 'error'); } catch {}
}

export async function readTaskContext({ cli, database, task }) {
  if (!isAbsolute(cli || '') || !isAbsolute(database || '') || !/^[1-9][0-9]*$/.test(task || '') || !positive(Number(task))) throw unavailable();
  try {
    // No default database, missing-path initialization, shell expansion, or business mutation.
    if (!(await stat(database)).isFile() || !(await stat(cli)).isFile()) throw unavailable();
    const { stdout } = await execute(cli, ['--database', database, '--json', 'task', 'context', task, '--require-read-only'], {
      timeout: 15000, maxBuffer: 8 * 1024 * 1024, encoding: 'buffer', windowsHide: true,
    });
    const result = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(stdout));
    const data = result.data;
    if (result.schemaVersion !== 2 || result.ok !== true || result.error !== null || !Array.isArray(result.warnings)
      || data?.task?.id !== Number(task) || !positive(data.task.version)
      || !(data.task.projectId === null || positive(data.task.projectId))
      || data.sessionRules?.formatVersion !== 1 || !Array.isArray(data.sessionRules.rules)) throw unavailable();
    const ids = new Set();
    for (const rule of data.sessionRules.rules) {
      if (!positive(rule.id) || ids.has(rule.id) || !positive(rule.revision) || rule.status !== 'active'
        || rule.contentVersion !== 1 || !text(rule.content?.name) || !text(rule.content?.body)
        || !Array.isArray(rule.content.sources)
        || !(rule.scope === 'global' && rule.projectId === null
          || rule.scope === 'project' && positive(rule.projectId) && rule.projectId === data.task.projectId)) throw unavailable();
      ids.add(rule.id);
      for (const source of rule.content.sources) {
        if (!['explicit', 'inferred'].includes(source.kind) || !text(source.evidence)
          || !(source.taskId === null && source.taskVersion === null || positive(source.taskId) && positive(source.taskVersion))) throw unavailable();
      }
    }
    // Preserve the entire envelope, including full rule body/provenance and warnings.
    return result;
  } catch { throw unavailable(); }
}

export function registerTaskContext(pi) {
  for (const [name, description] of [
    ['steward-context-task', 'Explicit numeric task ID for this fresh Pi session; read-only, no claim'],
    ['steward-context-cli', 'Absolute path to a compatible taskctl executable'],
    ['steward-context-database', 'Absolute path to an existing compatible database'],
  ]) pi.registerFlag(name, { type: 'string', description });
  let sessionId;
  let cwd;
  let validSession = false;
  let snapshot;
  let generation = 0;
  const configured = () => ({ task: pi.getFlag('steward-context-task'), cli: pi.getFlag('steward-context-cli'), database: pi.getFlag('steward-context-database') });
  const enabled = () => Object.values(configured()).some(v => v !== undefined);
  const matches = (value, ctx) => validSession && value && value.sessionId === sessionId && value.cwd === cwd
    && sessionId === ctx.sessionManager.getSessionId() && cwd === ctx.cwd;
  pi.on('session_start', (event, ctx) => {
    validSession = false;
    snapshot = undefined;
    generation++;
    sessionId = ctx.sessionManager.getSessionId();
    cwd = ctx.cwd;
    // Never re-target an existing conversation, /new, /resume, /fork or /reload.
    validSession = event.reason === 'startup' && !ctx.sessionManager.getEntries().some(e => e.type === 'message');
  });
  pi.on('session_shutdown', () => { validSession = false; snapshot = undefined; generation++; });
  pi.on('input', async (event, ctx) => {
    snapshot = undefined;
    const requestGeneration = ++generation;
    try {
      const config = configured();
      if (!Object.values(config).some(v => v !== undefined)) return { action: 'continue' };
      if (!validSession || sessionId !== ctx.sessionManager.getSessionId() || cwd !== ctx.cwd) throw unavailable();
      const expectedSession = sessionId, expectedCwd = cwd;
      const context = await readTaskContext(config);
      // Recheck the captured identity after IO, not just mutable extension state.
      if (!validSession || generation !== requestGeneration || sessionId !== expectedSession || cwd !== expectedCwd
        || expectedSession !== ctx.sessionManager.getSessionId() || expectedCwd !== ctx.cwd) throw unavailable();
      snapshot = { sessionId: expectedSession, cwd: expectedCwd, context };
      // Input runs BEFORE skill/template expansion. Leave text/images completely untouched.
      return { action: 'continue' };
    } catch {
      // Throwing extension hooks does NOT stop Pi: explicitly handle the input instead.
      reportBlocked(ctx);
      return { action: 'handled' };
    }
  });
  // This runs after Pi expands skills/templates and before the model call. The snapshot is
  // ephemeral: never rewrite user text or persist a stale rules message into the session.
  pi.on('context', async (event, ctx) => {
    try {
      if (!enabled()) return;
      if (!matches(snapshot, ctx)) throw unavailable();
      const content = `${CONTEXT_MARKER}${JSON.stringify(snapshot.context)}\n\nOnly this snapshot supplies current effective rules. Read applicable repository AGENTS independently. Historical source versions are references, not current-version assertions. This snapshot does not claim/resume, authorize deployment, close tasks, or override the user's request.`;
      return { messages: [...event.messages, { role: 'user', content: [{ type: 'text', text: content }], timestamp: Date.now() }] };
    } catch {
      // The agent's signal is active here. Do not await its idle promise inside its own loop.
      void ctx.abort();
      reportBlocked(ctx);
      return { messages: [] };
    }
  });
}
