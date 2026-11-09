// buddy.tools: bash with the exact environment (installed after CodingTools,
// so its bash replaces theirs); read/write/edit stay CodingTools'. Every
// command also gets the host API's address, this sidecar's agent token and the
// request id of the turn that ran it (BUDDY_TURN), so `buddy say …` reaches
// the right bubble (plan §2 "Host API auth").

import { defineExtension, type ToolRegistration } from "@earendil-works/pi-durable";
import { createBashTool } from "@earendil-works/pi-durable/tools";
import { shellEnv } from "../../env.ts";
import type { Init } from "../../protocol.ts";
import { currentTrigger } from "../gate/index.ts";

/** The tools the model is offered, in order. */
export const MODEL_TOOLS = ["bash", "read", "write", "edit"];

/** No `bash` on Windows until a shell (busybox-w32, D10) is configured through `shellPath`. */
export function modelToolNames(init: Init): string[] {
  return MODEL_TOOLS.filter((n) => !(n === "bash" && init.platform === "windows" && !init.shellPath));
}

/** The exact env of one command: Rust's shell env plus the BUDDY_* variables. */
export function commandEnv(init: Init, turn: string): Record<string, string> {
  const env = shellEnv(init);
  if (init.host) {
    env.BUDDY_API = init.host.url;
    env.BUDDY_TOKEN = init.host.token;
  }
  if (turn) env.BUDDY_TURN = turn;
  return env;
}

export function createTools(init: Init) {
  const bash = createBashTool({
    prepare: (e) => {
      e.inheritEnv = false;
      e.env = commandEnv(init, currentTrigger().requestId);
    },
  }) as unknown as ToolRegistration;
  return defineExtension({ name: "buddy.tools", tools: modelToolNames(init).includes("bash") ? [bash] : [] });
}
