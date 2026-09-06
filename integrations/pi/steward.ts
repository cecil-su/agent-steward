import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { register } from "./steward.mjs";

export default function (pi: ExtensionAPI) {
  register(pi);
}
