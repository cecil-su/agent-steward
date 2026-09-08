// Test-only explicit -e entrypoint. Never copy into an auto-discovered extension directory.
import { createPiEvidenceCollector } from "../pi/context-evidence.mjs";
export default function (pi) {
  const collector = createPiEvidenceCollector(pi, { version: process.env.STEWARD_TEST_PI_VERSION });
  pi.on("session_start", (_event, ctx) => collector.beginSession(ctx));
  pi.on("session_shutdown", () => collector.invalidate());
  pi.registerCommand("steward-evidence-probe", {
    description: "Isolated test-only metadata observation; no model call",
    handler: (_args, ctx) => {
      const result = collector.collectBase(ctx);
      console.log("STEWARD_TEST_EVIDENCE=" + JSON.stringify(result));
    },
  });
}
