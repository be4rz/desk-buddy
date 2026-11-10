// buddy.tools: bash with the exact environment (installed after CodingTools,
// so its bash replaces theirs); read/write/edit stay CodingTools'. Every
// command of the root conversation also gets the host API's address, this
// sidecar's agent token and the request id of the turn that ran it
// (BUDDY_TURN), so `buddy say …` reaches the right bubble (plan §2 "Host API
// auth"). A helper conversation's command never gets them.

import { defineExtension, type ToolRegistration } from "@earendil-works/pi-durable";
import { createBashTool } from "@earendil-works/pi-durable/tools";
import { takesImages } from "../../current.ts";
import { shellEnv } from "../../env.ts";
import type { Init, ModelRef } from "../../protocol.ts";
import { rootId } from "../core/index.ts";
import { currentTrigger } from "../gate/index.ts";

/** The tools the model is offered, in order. `look` only while the model takes images. */
export const MODEL_TOOLS = ["bash", "read", "write", "edit", "look", "web_fetch", "web_search", "task"];

/** No `bash` on Windows until a shell (busybox-w32, D10) is configured through `shellPath`. */
export const hasBash = (init: Init) => !(init.platform === "windows" && !init.shellPath);

export function modelToolNames(init: Init, model: ModelRef | null = init.model): string[] {
  return MODEL_TOOLS.filter((n) => {
    if (n === "bash") return hasBash(init);
    if (n === "look") return takesImages(model);
    return true;
  });
}

/** The exact env of one command: Rust's shell env, plus the BUDDY_* variables for the root conversation's. */
export function commandEnv(init: Init, turn: string, root = true): Record<string, string> {
  const env = shellEnv(init);
  if (!root) return env;
  if (init.host) {
    env.BUDDY_API = init.host.url;
    env.BUDDY_TOKEN = init.host.token;
  }
  if (turn) env.BUDDY_TURN = turn;
  return env;
}

export function createTools(init: Init) {
  const bash = createBashTool({
    prepare: (e, api) => {
      e.inheritEnv = false;
      e.env = commandEnv(init, currentTrigger().requestId, String(api.conversationId) === rootId());
    },
  }) as unknown as ToolRegistration;
  return defineExtension({ name: "buddy.tools", tools: hasBash(init) ? [bash] : [] });
}
