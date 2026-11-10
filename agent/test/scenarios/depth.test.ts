// harness-depth (AC7), sidecar half, on the scripted fake model:
// capping, clearing and compaction; the web tools against a local
// DuckDuckGo-HTML fixture (taint first); `look` offered by model input;
// `task` helpers (web-only, at most 3 at once); durable reminders that fire
// exactly once across a kill -9.

import { afterAll, expect, test } from "bun:test";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Driver, FAKE_MODELS, type Rec, toolResults } from "../support/driver.ts";
import { type RecordedRequest, startFake } from "../support/fake.ts";
import { DDG_HTML } from "../support/ddg.ts";

const fake = startFake({ script: [] });
afterAll(() => fake.stop());

const site = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  fetch(req) {
    const u = new URL(req.url);
    if (u.pathname === "/html/") return new Response(DDG_HTML.replace("Polar", `Polar (${u.searchParams.get("q")})`), { headers: { "content-type": "text/html" } });
    if (u.pathname === "/page") return new Response("<html><head><title>Bears</title><script>steal()</script></head><body><p>Polar bears are <b>white</b>.</p></body></html>", { headers: { "content-type": "text/html; charset=utf-8" } });
    if (u.pathname === "/hop") return new Response(null, { status: 302, headers: { location: "/page" } });
    return new Response("nope", { status: 404 });
  },
});
afterAll(() => site.stop(true));
const SITE = `http://127.0.0.1:${site.port}`;
const WEB = { web: { searchUrl: `${SITE}/html/`, allowLoopback: true } };

type Body = { messages: { role: string; content: unknown; tool_call_id?: string }[]; tools?: { function: { name: string } }[] };
const bodies = (since = 0): Body[] =>
  fake
    .requests()
    .slice(since)
    .filter((r: RecordedRequest) => r.path.endsWith("/chat/completions"))
    .map((r) => JSON.parse(r.body));
const offered = (b: Body) => (b.tools ?? []).map((t) => t.function.name);
const firstUser = (b: Body) => {
  const m = b.messages.find((m) => m.role === "user");
  return typeof m?.content === "string" ? m.content : JSON.stringify(m?.content ?? "");
};

async function session(opts: Partial<ConstructorParameters<typeof Driver>[0]> = {}) {
  const d = new Driver({ fakeUrl: fake.url, ...opts });
  const ready = await d.start();
  expect(ready.t).toBe("ready");
  return d;
}

test("a 500-line result keeps its first 200 lines and points at the full text", async () => {
  fake.setScript([{ match: "count", reply: { toolCalls: [{ name: "bash", args: { command: "seq 1 500" } }] }, times: 1 }]);
  const d = await session();
  await d.prompt("count to 500");
  await d.waitRunEnd(0, 20000);
  const [r] = toolResults(await d.snapshot());
  await d.shutdown();
  const footer = r!.text.match(/\[output capped: full text in (\.buddy\/outputs\/[A-Za-z0-9_.-]+\.txt)\]$/);
  expect(footer).not.toBeNull();
  expect(r!.text).toContain("\n200\n");
  expect(r!.text).not.toContain("\n201\n");
  const full = readFileSync(join(d.dir, footer![1]!), "utf8");
  expect(full.trim().split("\n")).toHaveLength(500);
}, 30000);

test("past 60% of the window the oldest results are cleared, a batch at a time", async () => {
  const calls = Array.from({ length: 10 }, (_, i) => ({ name: "bash", args: { command: `printf 'r${i} '; head -c 6000 /dev/zero | tr '\\0' x` } }));
  fake.setScript([{ match: "fill", reply: { toolCalls: calls }, times: 1 }]);
  const models = [{ ...FAKE_MODELS[0]!, contextWindow: 20000 }];
  const since = fake.requests().length;
  const d = await session({ models, settings: { context: { clearBatch: 2 } } });
  await d.prompt("fill the context");
  await d.waitRunEnd(0, 30000);
  const entries = await d.snapshot();
  await d.shutdown();
  const after = bodies(since).at(-1)!;
  const results = after.messages.filter((m) => m.role === "tool");
  expect(results).toHaveLength(10);
  const text = (m: { content: unknown }) => (typeof m.content === "string" ? m.content : JSON.stringify(m.content));
  const cleared = results.map((m, i) => (text(m).includes("[cleared]") ? i : -1)).filter((i) => i >= 0);
  expect(cleared.length).toBeGreaterThan(0);
  expect(cleared.length % 2).toBe(0);
  expect(cleared).toEqual(Array.from({ length: cleared.length }, (_, i) => i)); // the oldest
  expect(text(results[9]!)).toContain("r9 ");
  // Only the request changes: the transcript keeps every result.
  expect(toolResults(entries).every((r) => r.text.includes("xxxx"))).toBe(true);
}, 40000);

test("compaction runs at 80% of a 1 000-token window", async () => {
  const long = "Polar bears are marine mammals. ".repeat(60);
  fake.setScript([
    { match: "first", reply: { text: long, usage: { input: 900, output: 400 } }, times: 1 },
    { match: /summar/i, reply: { text: "Summary: we talked about polar bears." } },
  ]);
  const models = [{ ...FAKE_MODELS[0]!, contextWindow: 1000, maxTokens: 500 }];
  const d = await session({ models });
  await d.prompt("first question");
  await d.waitRunEnd(0, 20000);
  const n = d.records.length;
  await d.prompt("second question");
  await d.waitRunEnd(n, 30000);
  const kinds = (await d.snapshot()).map((e: Rec) => e.kind);
  await d.shutdown();
  expect(kinds).toContain("pi.compaction");
}, 40000);

test("web_search reads the fixture, marks the conversation first, and records taint_set once", async () => {
  fake.setScript([
    { match: "search", reply: { toolCalls: [{ name: "web_search", args: { query: "polar bears" } }] }, times: 1 },
    { match: "Polar", reply: { toolCalls: [{ name: "web_fetch", args: { url: `${SITE}/hop` } }] }, times: 1 },
  ]);
  const d = await session({ settings: WEB });
  await d.prompt("search the web");
  await d.waitRunEnd(0, 20000);
  const entries = await d.snapshot();
  await d.shutdown();
  const [search, page] = toolResults(entries);
  expect(search!.isError).toBe(false);
  expect(search!.text).toContain("1. Polar (polar bears) & bears\n   https://example.org/bears?a=1&b=2\n   Polar bears live in the Arctic.");
  expect(search!.text).not.toContain("y.js");
  expect(page!.isError).toBe(false);
  expect(page!.text).toContain("Polar bears are white.");
  expect(page!.text).not.toContain("steal()");
  const taints = d.reqs.filter((r) => r.op === "policy.taint");
  expect(taints.map((t) => t.source)).toEqual(["web_search", "web_fetch"]);
  expect(taints[0]).toMatchObject({ detail: "polar bears" });
  // The taint request came before the result was committed.
  const events = d.events().filter((e) => e.type === "taint_set");
  expect(events).toHaveLength(1);
  expect(events[0]).toMatchObject({ source: "web_search", detail: "polar bears" });
  // Both tools went through the gate.
  expect(d.reqs.filter((r) => r.op === "policy.decide").map((r) => r.tool)).toEqual(["web_search", "web_fetch"]);
}, 30000);

test("non-happy: no taint mark, no content; loopback refused without the test hook", async () => {
  fake.setScript([
    { match: "search", reply: { toolCalls: [{ name: "web_search", args: { query: "secret plans" } }] }, times: 1 },
    { match: "withheld", reply: { toolCalls: [{ name: "web_fetch", args: { url: `${SITE}/page` } }] }, times: 1 },
  ]);
  const d = await session({ settings: { web: { searchUrl: `${SITE}/html/`, allowLoopback: true } }, handlers: { decide: () => ({ decision: "allow" }), taint: () => Promise.reject(new Error("policy file locked")) } });
  await d.prompt("search");
  await d.waitRunEnd(0, 20000);
  const [r] = toolResults(await d.snapshot());
  await d.shutdown();
  expect(r!.isError).toBe(true);
  expect(r!.text).toContain("withheld");
  expect(r!.text).not.toContain("Arctic");

  fake.setScript([{ match: "fetch", reply: { toolCalls: [{ name: "web_fetch", args: { url: `${SITE}/page` } }] }, times: 1 }]);
  const e = await session();
  await e.prompt("fetch it");
  await e.waitRunEnd(0, 20000);
  const [l] = toolResults(await e.snapshot());
  await e.shutdown();
  expect(l!.isError).toBe(true);
  expect(l!.text).toContain("private network");
  expect(e.reqs.some((q) => q.op === "policy.taint")).toBe(false);
}, 40000);

// 1×1 transparent PNG.
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=", "base64");

test("look is offered only while the model takes images", async () => {
  const models = [FAKE_MODELS[0]!, { ...FAKE_MODELS[0]!, id: "fake-eyes", name: "Fake Eyes", input: ["text", "image"] as ("text" | "image")[] }];
  fake.setScript([{ match: "picture", reply: { toolCalls: [{ name: "look", args: { path: "bear.png" } }] }, times: 1 }]);
  const since = fake.requests().length;
  const d = await session({ models });
  writeFileSync(join(d.dir, "bear.png"), PNG);
  await d.prompt("hello");
  await d.waitRunEnd(0, 20000);
  expect(offered(bodies(since).at(-1)!)).not.toContain("look");
  expect(offered(bodies(since).at(-1)!)).toEqual(expect.arrayContaining(["bash", "read", "web_fetch", "web_search", "task"]));
  expect((await d.send("set_model", { model: { provider: "fake", model: "fake-eyes" } })).ok).toBe(true);
  const n = d.records.length;
  await d.prompt("look at the picture");
  await d.waitRunEnd(n, 20000);
  const entries = await d.snapshot();
  await d.shutdown();
  expect(offered(bodies(since).at(-1)!)).toContain("look");
  const result = entries.flatMap((e: Rec) => e.model ?? []).find((m: Rec) => m.role === "toolResult" && m.toolName === "look");
  expect(result.isError).toBeFalsy();
  expect(result.content[0]).toMatchObject({ type: "image", mimeType: "image/png" });
  expect(d.reqs.find((r) => r.op === "policy.decide" && r.tool === "look")).toMatchObject({ args: { path: "bear.png" }, conversation: "root" });
}, 40000);

test("task: up to three web-only helpers at once; the fourth is refused", async () => {
  const calls = ["alpha", "beta", "gamma", "delta"].map((q) => ({ name: "task", args: { prompt: `research ${q}` } }));
  fake.setScript([
    { match: "four questions", reply: { toolCalls: calls }, times: 1 },
    { match: /^research (\w+)/, reply: { text: "The helper's answer, from the web.", delayMs: 400 } },
  ]);
  const since = fake.requests().length;
  const d = await session({ settings: { ...WEB, subagents: { max: 3 } }, host: { url: "http://127.0.0.1:9", token: "tok-must-not-leak" } });
  await d.prompt("four questions please");
  await d.waitRunEnd(0, 40000);
  const entries = await d.snapshot();
  await d.shutdown();
  const results = toolResults(entries);
  expect(results).toHaveLength(4);
  const ok = results.filter((r) => !r.isError);
  const refused = results.filter((r) => r.isError);
  expect(ok).toHaveLength(3);
  expect(ok.every((r) => r.text.includes("The helper's answer"))).toBe(true);
  expect(refused).toHaveLength(1);
  expect(refused[0]!.text).toContain("3 helpers are already working");
  // Each helper was offered exactly the web tools, with its own instructions; no shell, so no BUDDY_* env.
  const helpers = bodies(since).filter((b) => firstUser(b).startsWith("research "));
  expect(helpers.length).toBe(3);
  for (const b of helpers) {
    expect(offered(b).sort()).toEqual(["web_fetch", "web_search"]);
    expect(JSON.stringify(b)).not.toContain("tok-must-not-leak");
    expect(JSON.stringify(b.messages[0])).toContain("research helper");
  }
}, 60000);

test("a reminder fires once, also across a kill -9 and restarts; list and cancel", async () => {
  fake.setScript([]);
  const d = await session();
  const id = "r_0123456789ab";
  const atMs = Date.now() + 2500;
  const added = await d.send("remind.add", { reminderId: id, atMs, text: "stretch" });
  expect(added).toMatchObject({ ok: true, result: { id, atMs, text: "stretch" } });
  // Idempotent on id.
  expect((await d.send("remind.add", { reminderId: id, atMs, text: "stretch" })).result).toMatchObject({ id, duplicate: true });
  expect((await d.send("remind.add", { reminderId: "bad", atMs, text: "x" })).error).toMatchObject({ code: "bad_request" });
  // A second one, cancelled before it fires.
  await d.send("remind.add", { reminderId: "r_bbbbbbbbbbbb", atMs: Date.now() + 1500, text: "never" });
  const list = (await d.send("remind.list")).result.reminders;
  expect(list.map((r: Rec) => r.id)).toEqual(["r_bbbbbbbbbbbb", id]);
  expect((await d.send("remind.cancel", { reminderId: "r_bbbbbbbbbbbb" })).result).toMatchObject({ cancelled: true });
  expect((await d.send("remind.cancel", { reminderId: "r_cccccccccccc" })).result).toEqual({ cancelled: false });
  // A sleeping reminder never makes the bear busy.
  expect(d.statuses().every((s) => !s.busy)).toBe(true);

  await d.kill9();
  await new Promise((r) => setTimeout(r, Math.max(0, atMs - Date.now()) + 300));
  await d.start({ lastExit: "crash" });
  await d.waitFor((r) => r.t === "ev" && r.kind === "entry" && r.entry.data?.type === "reminder_fired", 10000);
  const fired = d.events().filter((e) => e.type === "reminder_fired");
  expect(fired).toHaveLength(1);
  expect(fired[0]).toMatchObject({ id, text: "stretch", atMs });
  // When it was set, for the bubble's "set N min ago" chip.
  expect(fired[0].setAtMs).toBeGreaterThan(atMs - 10000);
  expect(fired[0].setAtMs).toBeLessThan(atMs);
  // Only a sleeping reminder: nothing was cut off.
  expect(d.events().some((e) => e.type === "interrupted")).toBe(false);
  expect((await d.send("remind.list")).result.reminders).toEqual([]);
  await d.shutdown();

  await d.start({ lastExit: "clean" });
  await new Promise((r) => setTimeout(r, 1500));
  expect(d.events().filter((e) => e.type === "reminder_fired")).toHaveLength(0);
  const all = (await d.snapshot()).filter((e: Rec) => e.kind === "buddy.event" && e.data?.type === "reminder_fired");
  expect(all.map((e: Rec) => e.data.id)).toEqual([id]);
  await d.shutdown();
  expect(existsSync(d.storagePath)).toBe(true);
}, 40000);
