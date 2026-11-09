// Extension install order. The gate is last, so it decides on the final
// arguments of every call.

import { createRegistry, type ToolRegistration } from "@earendil-works/pi-durable";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import { createCore } from "./ext/core/index.ts";
import { GateExt, setDecideMs } from "./ext/gate/index.ts";
import { createTools, modelToolNames } from "./ext/tools/index.ts";
import type { Init } from "./protocol.ts";

export function buildRegistry(init: Init) {
  setDecideMs(init.settings.decideMs);
  const extensions = [CodingTools, createCore(init), createTools(init), GateExt];
  const registry = createRegistry();
  for (const e of extensions) registry.install(e);
  // The last installed tool of a name wins: buddy.tools' bash over CodingTools'.
  const byName = new Map<string, ToolRegistration>();
  for (const e of extensions) for (const t of (e.tools ?? []) as ToolRegistration[]) byName.set(t.name, t);
  const modelTools = modelToolNames(init)
    .map((n) => byName.get(n))
    .filter((t): t is ToolRegistration => !!t);
  return { registry, modelTools };
}
