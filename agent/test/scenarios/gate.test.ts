// S2 criterion 2 (sidecar half): every tool call asks Rust's policy first.
// allow runs; deny and ask→deny block with the reason; ask→allow runs after
// the answer; a policy error or no answer in time blocks (fail closed).

import { afterAll, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { Driver, type Handlers, toolResults, unanswered } from "../support/driver.ts";
import { startFake } from "../support/fake.ts";

const fake = startFake({ script: [{ match: "touch it", reply: { toolCalls: [{ name: "bash", args: { command: "touch ran.txt && echo touched" } }] } }] });
afterAll(() => fake.stop());

async function run(handlers: Handlers, settings = {}) {
  const d = new Driver({ fakeUrl: fake.url, handlers, settings });
  await d.start();
  await d.prompt("touch it");
  await d.waitRunEnd(0, 20000);
  const entries = await d.snapshot();
  await d.shutdown();
  return { d, result: toolResults(entries)[0], ran: existsSync(join(d.dir, "ran.txt")), entries };
}

test("allow: the call runs, and decide sees the call and its trigger", async () => {
  const { d, result, ran } = await run({ decide: () => ({ decision: "allow" }) });
  expect(ran).toBe(true);
  expect(result).toMatchObject({ isError: false });
  expect(result.text).toContain("touched");
  const decide = d.reqs.find((r) => r.op === "policy.decide")!;
  expect(decide).toMatchObject({ tool: "bash", args: { command: "touch ran.txt && echo touched" }, conversation: "root", depth: 0, trigger: { source: "chat" } });
  expect(decide.trigger.requestId).toMatch(/^u:/);
}, 30000);

test("deny: blocked with the policy's reason", async () => {
  const { result, ran, entries } = await run({ decide: () => ({ decision: "deny", reason: "Writing here is off limits." }) });
  expect(ran).toBe(false);
  expect(result).toMatchObject({ isError: true });
  expect(result.text).toContain("Writing here is off limits.");
  expect(unanswered(entries)).toEqual([]);
}, 30000);

test("ask → allow: approval_asked, then the call runs after the answer", async () => {
  const { d, result, ran } = await run({
    decide: () => ({ decision: "ask", approval: { title: "Run touch ran.txt?", suggest: "Shell(touch:*)" } }),
    wait: async () => {
      await new Promise((r) => setTimeout(r, 200));
      return { answer: "once", decision: "allow" };
    },
  });
  expect(ran).toBe(true);
  expect(result.isError).toBe(false);
  const ev = d.past.flat().concat(d.records).filter((r) => r.t === "ev" && r.kind === "entry" && r.entry.kind === "buddy.event").map((r) => r.entry.data);
  expect(ev.find((e) => e.type === "approval_asked")).toMatchObject({ title: "Run touch ran.txt?" });
  expect(ev.find((e) => e.type === "approval_answered")).toMatchObject({ answer: "once" });
  // A tool slot showed as running only after the answer.
  expect(d.reqs.map((r) => r.op)).toEqual(expect.arrayContaining(["policy.decide", "policy.wait"]));
}, 30000);

test("ask → deny: blocked with the person's reason", async () => {
  const { result, ran } = await run({ decide: () => ({ decision: "ask", approval: {} }), wait: () => ({ answer: "deny", decision: "deny", reason: "Not now, buddy." }) });
  expect(ran).toBe(false);
  expect(result.text).toContain("Not now, buddy.");
}, 30000);

test("fail closed: a policy error blocks the call", async () => {
  const { result, ran } = await run({
    decide: () => {
      throw new Error("policy exploded");
    },
  });
  expect(ran).toBe(false);
  expect(result.isError).toBe(true);
  expect(result.text).toContain("permission check failed");
  expect(result.text).toContain("policy exploded");
}, 30000);

test("fail closed: no answer within decideMs blocks the call", async () => {
  const { result, ran } = await run({ decide: () => new Promise(() => {}) }, { decideMs: 300 });
  expect(ran).toBe(false);
  expect(result.text).toContain("no answer within 300 ms");
}, 30000);

test("fail closed: an unknown decision blocks the call", async () => {
  const { result, ran } = await run({ decide: () => ({ decision: "maybe" }) });
  expect(ran).toBe(false);
  expect(result.text).toContain("don't allow");
}, 30000);
