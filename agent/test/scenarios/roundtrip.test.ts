// S2 criterion 1: a Bun sidecar round trip over the JSONL protocol, against
// the fake provider: ready → ack → status busy → streamed text → answer
// entry → status idle. Plus the protocol's refusals.

import { afterAll, expect, test } from "bun:test";
import { Driver, lastAnswer } from "../support/driver.ts";
import { startFake } from "../support/fake.ts";

const fake = startFake({
  script: [
    { match: "hello", reply: { text: "Hi there, I'm your desk buddy!" } },
    { match: "stream", reply: { text: "one two three four five six seven eight", delayMs: 40 } },
  ],
});
afterAll(() => fake.stop());

test("prompt → answer, with ready, ack, status and entry records", async () => {
  const d = new Driver({ fakeUrl: fake.url });
  const ready = await d.start();
  expect(ready).toMatchObject({ t: "ready", v: 1 });
  const requestId = `u:${crypto.randomUUID()}`;
  const ack = await d.prompt("hello", requestId);
  expect(ack).toMatchObject({ ok: true, result: { duplicate: false } });
  await d.waitRunEnd();
  expect(lastAnswer(d.entries())).toBe("Hi there, I'm your desk buddy!");
  const phases = d.statuses().map((s) => s.phase);
  expect(phases[0]).toBe("idle");
  expect(phases).toContain("thinking");
  expect(phases.at(-1)).toBe("idle");
  // The input entry carries the request id; `seq` only grows.
  expect(d.entries().find((e) => e.kind === "buddy.input")?.data.requestId).toBe(requestId);
  const seqs = d.records.filter((r) => r.t === "ev").map((r) => r.seq);
  expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
  // No key reached the sidecar's own records.
  expect(JSON.stringify(d.records)).not.toContain("sk-fake-credential-7c1f");
  await d.shutdown();
}, 30000);

test("a slow answer streams as text deltas that add up to the answer", async () => {
  const d = new Driver({ fakeUrl: fake.url, settings: {} });
  await d.start();
  await d.prompt("stream please");
  await d.waitRunEnd();
  const texts = d.records.filter((r) => r.t === "ev" && r.kind === "text");
  let joined = "";
  for (const t of texts) joined = t.reset ? t.delta : joined + t.delta;
  const answer = lastAnswer(d.entries());
  expect(answer).toBe("one two three four five six seven eight");
  // Partials are throttled, so streaming may or may not show; what does show is a prefix of the answer.
  expect(answer.startsWith(joined)).toBe(true);
  await d.shutdown();
}, 30000);

test("refusals: a bad request id, an empty prompt, an unknown op; a resent request id is one input", async () => {
  const d = new Driver({ fakeUrl: fake.url });
  await d.start();
  expect(await d.prompt("hello", "not-an-id")).toMatchObject({ ok: false, error: { code: "bad_request" } });
  expect(await d.send("prompt", { requestId: `u:${crypto.randomUUID()}`, text: "  " })).toMatchObject({ ok: false, error: { code: "bad_request" } });
  expect(await d.send("frobnicate")).toMatchObject({ ok: false, error: { code: "unknown_op" } });
  const id = `u:${crypto.randomUUID()}`;
  const first = await d.prompt("hello", id);
  const again = await d.prompt("hello", id);
  expect(again.result).toEqual({ submissionId: first.result.submissionId, duplicate: true });
  await d.waitRunEnd();
  await d.shutdown();
}, 30000);

test("a wrong protocol version is fatal", async () => {
  const d = new Driver({ fakeUrl: fake.url });
  const orig = d.init.bind(d);
  d.init = (s) => ({ ...orig(s), v: 99 });
  const r = await d.start();
  expect(r).toMatchObject({ t: "fatal", code: "bad_version" });
  expect(await d.exited).toBe(1);
}, 30000);
