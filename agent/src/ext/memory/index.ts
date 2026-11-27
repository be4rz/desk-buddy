// buddy.memory: the sidecar's part of the memory feature. Rust owns the store and every rule; this makes the model
// calls it asks for: `memory.extract` (p1) and `memory.reflect` (p2). (The "What I know about you" section itself lives in ext/core/prompt.ts.)

import type { Conversation } from "@earendil-works/pi-durable";
import { defineCommand } from "../core/index.ts";
import { extract } from "./extract.ts";
import { reflect } from "./reflect.ts";

export function bindMemory(root: Conversation) {
  defineCommand("memory.extract", (args) => extract(root, args));
  defineCommand("memory.reflect", (args) => reflect(args));
}
