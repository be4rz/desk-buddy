// S2 criterion 4: a sidecar killed mid-run. The next one records
// `interrupted` and resume() continues the run: a cut-off unsafe call gets
// its interrupted result, a cut-off generation runs again (maxRetries 1), and
// the run goes on without a new prompt. No tool call is left without a result.

import { afterAll, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ALLOW_ALL, Driver, lastAnswer, toolResults, unanswered } from "../support/driver.ts";
import { startFake } from "../support/fake.ts";

const fake = startFake({
  script: [
    { match: "slow shell", reply: { toolCalls: [{ name: "bash", args: { command: "echo $$ > cmd.pid; echo started; sleep 20; echo finished" } }] } },
    { match: "slow answer", reply: { text: "A long answer that streams slowly, word by word, on and on.", delayMs: 400 }, times: 1 },
    { match: "slow answer", reply: { text: "The answer, again." } },
  ],
});
afterAll(() => fake.stop());

test("killed mid-tool: interrupted result, the run continues", async () => {
  const d = new Driver({ fakeUrl: fake.url, handlers: ALLOW_ALL });
  await d.start();
  const ack = await d.prompt("slow shell");
  await d.waitFor((r) => r.t === "ev" && r.kind === "status" && r.status.phase === "tool" && r.status.tools.length > 0);
  await new Promise((r) => setTimeout(r, 400));
  await d.kill9();
  // pi-durable runs each command in its own process group, so it outlives a SIGKILLed sidecar: reap it.
  const pgid = Number(readFileSync(join(d.dir, "cmd.pid"), "utf8"));
  try {
    process.kill(-pgid, "SIGKILL");
  } catch {}

  const before = fake.requests().length;
  await d.start({ lastExit: "crash" });
  await d.waitFor((r) => r.t === "ev" && r.kind === "status" && !r.status.busy && d.records.indexOf(r) > 1, 20000);
  expect(d.events().find((e) => e.type === "interrupted")).toMatchObject({ by: "crash", submissionId: ack.result.submissionId });
  // The run went on to the model without a new prompt.
  expect(fake.requests().length).toBe(before + 1);
  const entries = await d.snapshot();
  expect(unanswered(entries)).toEqual([]);
  expect(toolResults(entries)[0].isError).toBe(true);
  await d.shutdown();
}, 40000);

test("killed mid-generation: the generation runs again", async () => {
  const d = new Driver({ fakeUrl: fake.url });
  await d.start();
  await d.prompt("slow answer");
  await d.waitFor((r) => r.t === "ev" && r.kind === "status" && r.status.busy);
  await new Promise((r) => setTimeout(r, 700));
  await d.kill9();
  await d.start({ lastExit: "quit" });
  await d.waitFor((r) => r.t === "ev" && r.kind === "status" && !r.status.busy && d.records.indexOf(r) > 1, 20000);
  expect(d.events().find((e) => e.type === "interrupted")?.by).toBe("quit");
  const entries = await d.snapshot();
  expect(lastAnswer(entries)).toBe("The answer, again.");
  await d.shutdown();
  // A clean reopen with nothing unfinished records nothing.
  await d.start({ lastExit: "clean" });
  await new Promise((r) => setTimeout(r, 300));
  expect(d.events().filter((e) => e.type === "interrupted")).toEqual([]);
  await d.shutdown();
}, 40000);
