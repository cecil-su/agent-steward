import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { isAbsolute } from "node:path";

// No shell, transcript reads, message persistence, or host control responses.
export function register(pi, env = process.env) {
  pi.registerCommand("steward-session", {
    description: "Show this pi session ID for explicit Steward binding",
    handler: async (_args, ctx) => {
      ctx.ui.notify(`pi session: ${ctx.sessionManager.getSessionId()}`, "info");
    },
  });
  const { STEWARD_HOOK_BIN: bin, STEWARD_DATABASE: database,
    STEWARD_SESSION: session, STEWARD_EXTERNAL_SESSION: external } = env;
  const configured = bin && isAbsolute(bin) && database && isAbsolute(database) && session && external;
  let warned = false;
  function warn(ctx) {
    if (warned) return;
    warned = true;
    const message = "Steward: observation unavailable; check explicit session binding and adapter configuration.";
    if (ctx.hasUI) ctx.ui.notify(message, "warning");
    else console.error(message);
  }
  async function observe(kind, ctx) {
    try {
      if (!configured) { warn(ctx); return; }
      if (ctx.sessionManager.getSessionId() !== external) return;
      const payload = JSON.stringify({ eventId: `observation-${randomUUID()}`,
        kind, occurredAt: new Date().toISOString() });
      const ok = await new Promise((resolve) => {
        const child = spawn(bin, ["--database", database, "--session", session,
          "--source", "pi", "--external-session", external],
          { stdio: ["pipe", "ignore", "ignore"], shell: false });
        const timer = setTimeout(() => { child.kill("SIGKILL"); resolve(false); }, 2000);
        const finish = (ok) => { clearTimeout(timer); resolve(ok); };
        child.on("error", () => finish(false));
        child.on("close", code => finish(code === 0));
        child.stdin.on("error", () => {});
        child.stdin.end(payload);
      });
      if (!ok) warn(ctx);
    } catch { warn(ctx); }
  }
  pi.on("session_start", async (event, ctx) => {
    await observe(["resume", "reload"].includes(event.reason) ? "resumed" : "started", ctx);
  });
  pi.on("session_shutdown", async (_event, ctx) => { await observe("closed", ctx); });
  pi.on("agent_settled", async (_event, ctx) => { await observe("idle", ctx); });
  pi.on("message_end", async (event, ctx) => {
    if (event.message.role === "user") await observe("user_message", ctx);
    if (event.message.role === "assistant") await observe("assistant_message", ctx);
  });
  pi.on("tool_execution_start", async (_event, ctx) => { await observe("tool_call", ctx); });
  pi.on("tool_execution_end", async (event, ctx) => {
    await observe(event.isError ? "error" : "tool_result", ctx);
  });
}
