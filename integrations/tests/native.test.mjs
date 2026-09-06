import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import { register } from "../pi/steward.mjs";

const binary = resolve("target/debug/task-hook");
const cli = resolve("target/debug/taskctl");
const secret = "synthetic-private-prompt-and-token";
function fixture(source) {
  const dir = mkdtempSync(join(tmpdir(), "steward-native-"));
  const database = join(dir, "state.db");
  function task(...args) {
    const r = spawnSync(cli, ["--database", database, "--json", ...args], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    return JSON.parse(r.stdout);
  }
  task("task", "create");
  task("task", "claim", "1", "--session", "local", "--if-version", "1");
  task("session", "bind", "local", "--source", source, "--external-session", "external", "--if-version", "2");
  return { dir, database, task, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}
test("Codex native payloads remain passive, private, and session scoped", () => {
  const f = fixture("codex");
  try {
    function run(payload) {
      return spawnSync(binary, ["--database", f.database, "--session", "local", "--source", "codex",
        "--client", "codex", "--external-session", "external"], { input: JSON.stringify(payload), encoding: "utf8" });
    }
    for (const name of ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "Interrupt"]) {
      const result = run({ hook_event_name: name, session_id: "external", source: "startup",
        prompt: secret, tool_input: { token: secret }, transcript_path: "/must/not/read" });
      assert.equal(result.status, 0, result.stderr);
      assert.deepEqual(JSON.parse(result.stdout), {});
    }
    assert.equal(run({ hook_event_name: "Stop", session_id: "other" }).status, 0);
    assert.equal(run({ hook_event_name: "Unknown", session_id: "external" }).status, 0);
    const bad = run({ prompt: secret });
    assert.equal(bad.status, 1); // Never exit 2: that can block or steer Codex.
    assert.ok(!bad.stderr.includes(secret));
    const events = f.task("hook", "list", "local").data.events;
    assert.deepEqual(events.map(e => e.kind), ["started", "closed", "user_message", "tool_call", "tool_result", "idle", "idle"]);
    assert.equal(f.task("task", "show", "1").data.task.version, 3);
    assert.ok(!readFileSync(f.database).includes(Buffer.from(secret)));
  } finally { f.cleanup(); }
});
test("pi extension records native lifecycle without replacing messages or following session switches", async () => {
  const f = fixture("pi");
  try {
    const handlers = new Map();
    const warnings = [];
    const ctx = { hasUI: true, ui: { notify: m => warnings.push(m) }, sessionManager: { getSessionId: () => "external" } };
    register({ registerCommand: () => {}, on: (name, fn) => handlers.set(name, fn) }, { STEWARD_HOOK_BIN: binary,
      STEWARD_DATABASE: f.database, STEWARD_SESSION: "local", STEWARD_EXTERNAL_SESSION: "external" });
    for (const [name, payload] of [["session_start", { reason: "resume" }],
      ["message_end", { message: { role: "user", content: secret } }],
      ["message_end", { message: { role: "assistant", content: secret } }],
      ["tool_execution_start", { args: secret }], ["tool_execution_end", { result: secret, isError: false }],
      ["tool_execution_end", { result: secret, isError: true }], ["agent_settled", {}], ["session_shutdown", {}]]) {
      assert.equal(await handlers.get(name)(payload, ctx), undefined);
    }
    ctx.sessionManager.getSessionId = () => "different";
    await handlers.get("session_start")({ reason: "new" }, ctx);
    const events = f.task("hook", "list", "local").data.events;
    assert.deepEqual(events.map(e => e.kind), ["resumed", "user_message", "assistant_message", "tool_call", "tool_result", "error", "idle", "closed"]);
    assert.equal(f.task("task", "show", "1").data.task.version, 3);
    assert.equal(warnings.length, 0);
    assert.ok(!readFileSync(f.database).includes(Buffer.from(secret)));
  } finally { f.cleanup(); }
});
test("pi spawn failure is bounded, reported once, and never aborts the host", async () => {
  const handlers = new Map(), warnings = [];
  register({ registerCommand: () => {}, on: (n, f) => handlers.set(n, f) }, { STEWARD_HOOK_BIN: "/does-not-exist/task-hook",
    STEWARD_DATABASE: "/tmp/unused.db", STEWARD_SESSION: "local", STEWARD_EXTERNAL_SESSION: "external" });
  const ctx = { hasUI: true, ui: { notify: m => warnings.push(m) }, sessionManager: { getSessionId: () => "external" } };
  assert.equal(await handlers.get("agent_settled")({}, ctx), undefined);
  assert.equal(await handlers.get("agent_settled")({}, ctx), undefined);
  assert.equal(warnings.length, 1);
});
test("Codex config generator quotes shell metacharacters and sets bounded hooks", () => {
  const path = "/tmp/a 'b $(no-execution)/task-hook";
  const result = spawnSync("python3", ["integrations/codex/configure.py", "--binary", path,
    "--database", "/tmp/db with spaces", "--session", "local", "--external-session", "external"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const hooks = JSON.parse(result.stdout).hooks;
  assert.equal(Object.keys(hooks).length, 7);
  const command = hooks.Stop[0].hooks[0];
  assert.equal(command.timeout, 3);
  const parsed = spawnSync("python3", ["-c", "import shlex,json,sys;print(json.dumps(shlex.split(sys.argv[1])))", command.command], { encoding: "utf8" });
  assert.equal(JSON.parse(parsed.stdout)[0], path);
});
