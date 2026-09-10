import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerTaskContext } from "./task-context.mjs";

// Explicit candidate entry: pi --no-extensions -e /absolute/task-context.ts ...
export default function (pi: ExtensionAPI) {
  registerTaskContext(pi);
}
