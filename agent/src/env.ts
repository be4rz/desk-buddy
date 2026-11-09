// The shell environment tools run with: exactly `init.shellEnv`, built by Rust
// with the same rules as a terminal agent's. Nothing is inherited from this
// process, so a key in the sidecar's memory never reaches a command.

import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import type { Init } from "./protocol.ts";

export function shellEnv(init: Init): Record<string, string> {
  return { ...init.shellEnv };
}

export function executionEnv(init: Init, cwd: string | undefined) {
  return new NodeExecutionEnv({ cwd: cwd ?? init.workDir, shellPath: init.shellPath || undefined, shellEnv: shellEnv(init) });
}
