// Extension install order. The gate is last, so it decides on the final
// arguments of every call; buddy.context comes before it, so it caps every
// result the gate let through (and the gate's own blocks).

import { createRegistry, type ToolRegistration } from "@earendil-works/pi-durable";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import { createContext } from "./ext/context/index.ts";
import { createCore } from "./ext/core/index.ts";
import { GateExt, setDecideMs } from "./ext/gate/index.ts";
import { JobsExt } from "./ext/jobs/index.ts";
import { createLook } from "./ext/look/index.ts";
import { createTask } from "./ext/task/index.ts";
import { createTools, modelToolNames } from "./ext/tools/index.ts";
import { createWeb } from "./ext/web/index.ts";
import type { Init, ModelRef } from "./protocol.ts";

export function buildRegistry(init: Init) {
  setDecideMs(init.settings.decideMs);
  const web = createWeb(init);
  const extensions = [
    CodingTools,
    createCore(init),
    createTools(init),
    web,
    createLook(init),
    createTask(init, (web.tools ?? []) as ToolRegistration[]),
    JobsExt,
    createContext(init),
    GateExt,
  ];
  const registry = createRegistry();
  for (const e of extensions) registry.install(e);
  // The last installed tool of a name wins: buddy.tools' bash over CodingTools'.
  const byName = new Map<string, ToolRegistration>();
  for (const e of extensions) for (const t of (e.tools ?? []) as ToolRegistration[]) byName.set(t.name, t);
  /** The tools offered to the model with `model`: `look` only when it takes images. */
  const modelToolsFor = (model: ModelRef | null) =>
    modelToolNames(init, model)
      .map((n) => byName.get(n))
      .filter((t): t is ToolRegistration => !!t);
  return { registry, modelTools: modelToolsFor(init.model), modelToolsFor };
}
