import { createHash, randomUUID } from "node:crypto";

// Observation only. No fs, process execution, provider/auth access, logging, prompt mutation or persistence.
const fail = () => { throw new Error("Pi context evidence unavailable or changed; do not reuse"); };
const hash = text => createHash("sha256").update(text, "utf8").digest("hex");
function text(value, max = 4096) {
  if (typeof value !== "string" || value.length > max || !value.length || /[\u0000-\u001f\u007f]/u.test(value)) fail();
  return value;
}
function body(value, limit) {
  if (typeof value !== "string" || value.length > limit || Buffer.byteLength(value, "utf8") > limit) fail();
  return value;
}
function array(value, max) {
  if (!Array.isArray(value) || value.length > max) fail();
  return value;
}
function names(value) {
  const result = array(value, 256).map(v => text(v, 160));
  if (new Set(result).size !== result.length) fail();
  return result;
}
// Hash JSON metadata only. Functions/execution code are NOT fingerprinted or probed.
function canonical(value) {
  let nodes = 0;
  function visit(v, depth) {
    if (++nodes > 20000 || depth > 24) fail();
    if (v === null || typeof v === "boolean") return v;
    if (typeof v === "string") return body(v, 128 * 1024);
    if (typeof v === "number" && Number.isFinite(v)) return v;
    if (Array.isArray(v)) return v.map(x => visit(x, depth + 1));
    if (v && typeof v === "object" && [Object.prototype, null].includes(Object.getPrototypeOf(v))) {
      return Object.fromEntries(Object.keys(v).filter(key => v[key] !== undefined).sort().map(key => [key, visit(v[key], depth + 1)]));
    }
    fail();
  }
  const serialized = JSON.stringify(visit(value, 0));
  if (Buffer.byteLength(serialized, "utf8") > 128 * 1024) fail();
  return serialized;
}

/**
 * Explicitly owned by a future adapter; not an auto-loaded extension.
 * Call beginSession on session_start (including reload), invalidate on session_shutdown.
 * version comes from the verified Pi runtime/package, not from a model or evidence report.
 * Results are extension-local observations, NOT Rust HostEvidence or final-payload proofs.
 */
export function createPiEvidenceCollector(pi, { version, clock = Date.now, instanceId = randomUUID } = {}) {
  text(version, 160);
  let active;
  function identity(ctx) {
    return { piSessionId: text(ctx.sessionManager.getSessionId(), 160), cwd: text(ctx.cwd) };
  }
  function check(ctx) {
    const current = identity(ctx);
    if (!active || current.piSessionId !== active.piSessionId || current.cwd !== active.cwd) fail();
    return current;
  }
  function capture(options, ctx, point, prompt) {
    const current = check(ctx);
    if (!options || options.cwd !== current.cwd) fail();
    const selected = names(options.selectedTools);
    const activeTools = names(pi.getActiveTools());
    if (canonical([...selected].sort()) !== canonical([...activeTools].sort())) fail();
    let inputBytes = 0;
    const rules = array(options.contextFiles, 64).map(file => {
      const path = text(file.path);
      const content = body(file.content, 256 * 1024);
      const bytes = Buffer.byteLength(content, "utf8");
      inputBytes += bytes;
      if (inputBytes > 4 * 1024 * 1024) fail();
      return { path, loadedTextSha256: hash(content), loadedTextBytes: bytes };
    });
    if (new Set(rules.map(r => r.path)).size !== rules.length) fail();
    const toolNames = new Set();
    let toolBytes = 0;
    const tools = array(pi.getAllTools(), 256).map(tool => {
      const name = text(tool.name, 160);
      if (toolNames.has(name)) fail();
      toolNames.add(name);
      const descriptor = canonical({ description: body(tool.description, 128 * 1024), parameters: tool.parameters,
        promptGuidelines: tool.promptGuidelines ?? [], sourceInfo: tool.sourceInfo ?? null });
      toolBytes += Buffer.byteLength(descriptor, "utf8");
      if (toolBytes > 2 * 1024 * 1024) fail();
      return { name, active: activeTools.includes(name), metadataSha256: hash(descriptor), implementationVersion: null, configurationVerified: false };
    }).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
    if (activeTools.some(name => !toolNames.has(name))) fail();
    const optionalHash = value => value === undefined ? null : hash(body(value, 4 * 1024 * 1024));
    return { ...current, instanceId: active.instanceId, piVersion: version, point,
      selectedTools: selected, rules, tools,
      // Hash only: these fields may contain secrets or extension-added instructions.
      customPromptSha256: optionalHash(options.customPrompt), appendPromptSha256: optionalHash(options.appendSystemPrompt),
      chainedPromptSha256: optionalHash(prompt),
      promptMetadataSha256: hash(canonical({ toolSnippets: options.toolSnippets ?? {}, promptGuidelines: options.promptGuidelines ?? [], skills: options.skills ?? [] })) };
  }
  function collect(ctx, read, point) {
    try {
      const started = clock();
      if (!Number.isSafeInteger(started) || started < 0) fail();
      const firstInput = read();
      const first = capture(firstInput.options, ctx, point, firstInput.prompt);
      const secondInput = read();
      const second = capture(secondInput.options, ctx, point, secondInput.prompt);
      if (JSON.stringify(first) !== JSON.stringify(second)) fail();
      const ended = clock();
      if (!Number.isSafeInteger(ended) || ended < started || ended - started >= 60_000) fail();
      const result = { observationVersion: 1, ...first, observedAtMs: ended, expiresAtMs: started + 60_000,
        fingerprint: hash(JSON.stringify(first)), encoding: "pi-decoded-text-as-utf8",
        hostClaimVerified: false, reuseAllowed: false,
        blockers: ["NATIVE_FILE_IDENTITY_UNOBSERVED", "TOOL_IMPLEMENTATION_CONFIG_UNOBSERVED", "FINAL_PROVIDER_PAYLOAD_UNOBSERVED", "RULE_SCOPE_COVERAGE_UNPROVEN", "DEPENDENCY_COVERAGE_UNKNOWN"] };
      if (!Number.isSafeInteger(result.expiresAtMs) || Buffer.byteLength(JSON.stringify(result), "utf8") > 64_000) fail();
      return result;
    } catch { fail(); } // Never propagate exception text containing prompt/config material.
  }
  return {
    beginSession(ctx) {
      active = undefined;
      try { active = { ...identity(ctx), instanceId: text(instanceId(), 160) }; } catch { fail(); }
    },
    invalidate() { active = undefined; },
    collectBase(ctx) { return collect(ctx, () => ({ options: ctx.getSystemPromptOptions() }), "command-base-inputs"); },
    collectBeforeAgentStart(event, ctx) {
      return collect(ctx, () => ({ options: event.systemPromptOptions, prompt: event.systemPrompt }), "before-agent-start-handler");
    },
  };
}
