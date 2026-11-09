// The desk-buddy agent sidecar: reads `init`, then serves Rust's commands.

import { host } from "./host.ts";
import { CmdError, exit, type Init, records, send } from "./protocol.ts";

async function main() {
  const input = records()[Symbol.asyncIterator]();
  const first = await input.next();
  if (first.done || first.value.t !== "init") throw new CmdError("no_init", "The first line must be init.");
  await host(first.value as unknown as Init, input);
}

main().then(
  () => exit(0),
  (e) => {
    send({ t: "fatal", code: e instanceof CmdError ? e.code : "error", message: e instanceof Error ? e.message : String(e) });
    process.stderr.write(`${e instanceof Error ? (e.stack ?? e.message) : String(e)}\n`);
    return exit(1);
  },
);
