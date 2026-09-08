import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createPiEvidenceCollector } from "../pi/context-evidence.mjs";
const hash = value => createHash("sha256").update(value).digest("hex");
function fixture() {
  let session = "pi-session-1";
  const options = { cwd: "/fixture", contextFiles: [{ path: "/fixture/AGENTS.override.md", content: "private-rule-sentinel" }], selectedTools: ["read"], customPrompt: "private-system-sentinel" };
  const tools = [{ name: "read", description: "private-tool-sentinel", parameters: { type: "object", properties: {} }, sourceInfo: { source: "builtin" } }];
  const pi = { getActiveTools: () => ["read"], getAllTools: () => tools };
  const ctx = { cwd: "/fixture", sessionManager: { getSessionId: () => session }, getSystemPromptOptions: () => options };
  let generation = 0;
  const collector = createPiEvidenceCollector(pi, { version: "0.85.1", clock: () => 1000, instanceId: () => `instance-${++generation}` });
  collector.beginSession(ctx);
  return { options, tools, pi, ctx, collector, session: value => { session = value; } };
}
test("hash loaded inputs without disclosing text or claiming tool configuration", () => {
  const f = fixture(); const out = f.collector.collectBase(f.ctx);
  assert.equal(out.rules[0].loadedTextSha256, hash("private-rule-sentinel"));
  assert.equal(out.customPromptSha256, hash("private-system-sentinel"));
  assert.equal(out.tools[0].active, true);
  assert.equal(out.tools[0].implementationVersion, null);
  assert.equal(out.hostClaimVerified, false); assert.equal(out.reuseAllowed, false);
  assert.ok(!JSON.stringify(out).includes("sentinel"));
  assert.equal(out.fingerprint, f.collector.collectBase(f.ctx).fingerprint);
  f.tools[0].parameters.properties.path = { type: "string" };
  assert.notEqual(out.fingerprint, f.collector.collectBase(f.ctx).fingerprint);
});
test("handler-stage prompt differs from base inputs and does not mutate either", () => {
  const f = fixture(); const before = JSON.stringify(f.options);
  const event = { systemPromptOptions: f.options, systemPrompt: "private-chained-sentinel" };
  const out = f.collector.collectBeforeAgentStart(event, f.ctx);
  assert.equal(out.chainedPromptSha256, hash(event.systemPrompt));
  assert.notEqual(out.fingerprint, f.collector.collectBase(f.ctx).fingerprint);
  assert.equal(JSON.stringify(f.options), before);
  assert.equal(event.systemPrompt, "private-chained-sentinel");
});
test("missing APIs, virtual rules, empty lists and input limits are explicit", () => {
  const f = fixture(); f.options.contextFiles[0].path = "/virtual/AGENTS.md";
  assert.equal(f.collector.collectBase(f.ctx).rules[0].path, "/virtual/AGENTS.md");
  f.options.contextFiles = []; assert.equal(f.collector.collectBase(f.ctx).rules.length, 0);
  delete f.options.contextFiles; assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
  f.options.contextFiles = [{ path: "/fixture/rule", content: "x".repeat(256 * 1024 + 1) }];
  assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
});
test("reload, shutdown, session switch and cwd switch invalidate observation identity", () => {
  const f = fixture(); const first = f.collector.collectBase(f.ctx);
  f.collector.invalidate(); assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
  f.collector.beginSession(f.ctx);
  assert.notEqual(first.fingerprint, f.collector.collectBase(f.ctx).fingerprint);
  f.session("pi-session-2"); assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
  f.collector.beginSession(f.ctx); assert.equal(f.collector.collectBase(f.ctx).piSessionId, "pi-session-2");
  f.ctx.cwd = "/other"; assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
});
test("tool registration mismatch, metadata races, clock errors and exceptions fail without text leaks", () => {
  const f = fixture(); f.options.selectedTools = []; assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
  f.options.selectedTools = ["read"];
  let n = 0; f.pi.getAllTools = () => { f.tools[0].description = String(++n); return f.tools; };
  assert.throws(() => f.collector.collectBase(f.ctx), /unavailable/);
  f.pi.getAllTools = () => { throw new Error("private-exception-sentinel"); };
  assert.throws(() => f.collector.collectBase(f.ctx), e => !e.message.includes("sentinel"));
  f.ctx.sessionManager.getSessionId = () => { throw new Error("private-session-sentinel"); };
  assert.throws(() => f.collector.beginSession(f.ctx), e => !e.message.includes("sentinel"));
  const g = fixture(); let calls = 0;
  const collector = createPiEvidenceCollector(g.pi, { version: "0.85.1", clock: () => calls++ === 0 ? 1000 : 999 });
  collector.beginSession(g.ctx); assert.throws(() => collector.collectBase(g.ctx), /unavailable/);
});

// Explicit opt-in runtime contract test; no npm install, provider credentials, model prompt or persistent session.
test("real Pi 0.85.1 command API observes override precedence and disabled context loading", { skip: !process.env.STEWARD_TEST_PI_BIN }, () => {
  const root = mkdtempSync(join(tmpdir(), "steward-pi-evidence-"));
  try {
    const agent = join(root, "agent"); const project = join(root, "project");
    mkdirSync(agent); mkdirSync(project);
    writeFileSync(join(agent, "AGENTS.md"), "global fixture rule");
    writeFileSync(join(project, "AGENTS.override.md"), "override fixture rule");
    writeFileSync(join(project, "AGENTS.md"), "not selected AGENTS");
    writeFileSync(join(project, "CLAUDE.md"), "not selected CLAUDE");
    // Only OS process essentials are inherited; no auth/proxy/Pi-session environment is copied.
    const env = Object.fromEntries(["PATH", "Path", "SystemRoot", "SYSTEMROOT", "WINDIR", "TEMP", "TMP"].filter(k => process.env[k]).map(k => [k, process.env[k]]));
    Object.assign(env, { HOME: root, USERPROFILE: root, APPDATA: root, LOCALAPPDATA: root, PI_CODING_AGENT_DIR: agent, PI_OFFLINE: "1", PI_TELEMETRY: "0" });
    const bin = resolve(process.env.STEWARD_TEST_PI_BIN);
    const version = spawnSync(bin, ["--version"], { env, cwd: project, encoding: "utf8", timeout: 15000 });
    assert.equal(version.status, 0, "isolated Pi version invocation failed");
    env.STEWARD_TEST_PI_VERSION = version.stdout.trim();
    assert.equal(env.STEWARD_TEST_PI_VERSION, "0.85.1", "review API compatibility before using another Pi version");
    const probe = fileURLToPath(new URL("./pi-context-probe.ts", import.meta.url));
    function collect(extra = []) {
      const result = spawnSync(bin, ["--offline", "--no-session", "--no-extensions", "-e", probe, "--no-skills", "--no-prompt-templates", "--no-themes", "--no-tools", "--no-approve", ...extra, "-p", "/steward-evidence-probe"], { env, cwd: project, encoding: "utf8", timeout: 20000, maxBuffer: 128 * 1024 });
      assert.equal(result.status, 0, "isolated Pi command probe failed (no model request was intended)");
      const line = result.stdout.split(/\r?\n/).find(s => s.startsWith("STEWARD_TEST_EVIDENCE="));
      assert.ok(line, "Pi command did not expose a bounded observation");
      return JSON.parse(line.slice("STEWARD_TEST_EVIDENCE=".length));
    }
    const loaded = collect();
    assert.ok(loaded.rules.some(r => r.loadedTextSha256 === hash("global fixture rule")));
    assert.ok(loaded.rules.some(r => r.loadedTextSha256 === hash("override fixture rule")));
    assert.ok(loaded.rules.findIndex(r => r.loadedTextSha256 === hash("global fixture rule")) < loaded.rules.findIndex(r => r.loadedTextSha256 === hash("override fixture rule")));
    assert.ok(!loaded.rules.some(r => [hash("not selected AGENTS"), hash("not selected CLAUDE")].includes(r.loadedTextSha256)));
    assert.equal(loaded.reuseAllowed, false);
    assert.ok(loaded.tools.every(tool => !tool.active));
    const readOnly = collect(["--tools", "read"]);
    assert.deepEqual(readOnly.selectedTools, ["read"]);
    assert.equal(readOnly.tools.find(tool => tool.name === "read").active, true);
    assert.equal(readOnly.tools.find(tool => tool.name === "read").implementationVersion, null);
    assert.equal(collect(["--no-context-files"]).rules.length, 0);
    rmSync(join(project, "AGENTS.override.md"));
    const fallback = collect();
    assert.ok(fallback.rules.some(rule => rule.loadedTextSha256 === hash("not selected AGENTS")));
    assert.ok(!fallback.rules.some(rule => rule.loadedTextSha256 === hash("not selected CLAUDE")));
    assert.ok(!readdirSync(agent, { recursive: true }).some(name => String(name).endsWith(".jsonl")), "probe must not persist a session");
  } finally { rmSync(root, { recursive: true, force: true }); } // Own temporary fixtures only.
});
