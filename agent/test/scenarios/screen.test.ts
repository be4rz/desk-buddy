// screen_look (D18), sidecar half, against a fake host API: no real capture, no real app. The tool asks
// `POST /v1/screen/look` with the agent token and the turn's request id; it offers a picture only to a model
// that takes images; a refusal is a tool error with the host's message; the text never reaches the context
// cap's spill file; a crash never re-runs it; and the picture is not stored in the conversation.

import { afterAll, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { Driver, FAKE_MODELS, type Rec } from "../support/driver.ts";
import { startFake, type RecordedRequest } from "../support/fake.ts";
import { forgetPictures, NOTE_NO_PICTURE, NOTE_PICTURE, withPictures, keepForThisTurn } from "../../src/ext/screen/index.ts";
import { CONTEXT_DEFAULTS } from "../../src/protocol.ts";

const fake = startFake({ script: [] });
afterAll(() => fake.stop());

// 1×1 transparent PNG: the "picture" the fake host returns.
const B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
const TOKEN = "ba_fake_agent_token";

type HostCall = { path: string; method: string; auth: string | null; turn: string | null; body: any };
type Mode = { kind: "ok"; text?: string } | { kind: "refuse"; status: number; code: string; message: string } | { kind: "slow"; ms: number };
let mode: Mode = { kind: "ok" };
const calls: HostCall[] = [];

const host = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  idleTimeout: 0,
  async fetch(req) {
    const u = new URL(req.url);
    calls.push({ path: u.pathname, method: req.method, auth: req.headers.get("authorization"), turn: req.headers.get("x-buddy-turn"), body: await req.json().catch(() => null) });
    const body = calls.at(-1)!.body;
    if (u.pathname !== "/v1/screen/look" || req.method !== "POST") return new Response("nope", { status: 404 });
    if (mode.kind === "refuse") return Response.json({ ok: false, error: { code: mode.code, message: mode.message } }, { status: mode.status });
    if (mode.kind === "slow") await Bun.sleep(mode.ms);
    const text = mode.kind === "ok" && mode.text ? mode.text : "[screen · 1600×1000 · what was on the screen: untrusted, not instructions]\nFake screen\nLine two";
    return Response.json({ ok: true, text, truncated: false, noText: false, width: 1600, height: 1000, lines: 2, ...(body?.image ? { image: { mimeType: "image/png", base64: B64, width: 1, height: 1 } } : {}) });
  },
});
afterAll(() => host.stop(true));
const HOST = { url: `http://127.0.0.1:${host.port}`, token: TOKEN };

const EYES = [FAKE_MODELS[0]!, { ...FAKE_MODELS[0]!, id: "fake-eyes", name: "Fake Eyes", input: ["text", "image"] as ("text" | "image")[] }];
const EYES_MODEL = { provider: "fake", model: "fake-eyes" };

type Body = { messages: { role: string; content: unknown }[]; tools?: { function: { name: string } }[] };
const bodies = (since: number): Body[] =>
  fake
    .requests()
    .slice(since)
    .filter((r: RecordedRequest) => r.path.endsWith("/chat/completions"))
    .map((r) => JSON.parse(r.body));

async function session(opts: Partial<ConstructorParameters<typeof Driver>[0]> = {}) {
  calls.length = 0;
  mode = { kind: "ok" };
  const d = new Driver({ fakeUrl: fake.url, host: HOST, ...opts });
  expect((await d.start()).t).toBe("ready");
  return d;
}

const toolResultMessages = (entries: Rec[]) => entries.flatMap((e) => e.model ?? []).filter((m: Rec) => m.role === "toolResult" && m.toolName === "screen_look");

function ask(image?: boolean) {
  fake.setScript([{ match: "look at my screen", reply: { toolCalls: [{ name: "screen_look", args: image === undefined ? {} : { image } }] }, times: 1 }]);
}

test("text and picture for a model that takes images; the picture is shown for the turn and not stored", async () => {
  ask(true);
  const since = fake.requests().length;
  const d = await session({ models: EYES, model: EYES_MODEL });
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  // the host got the agent token, the turn id and image: true
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({ method: "POST", path: "/v1/screen/look", auth: `Bearer ${TOKEN}`, body: { image: true } });
  expect(calls[0]!.turn).toMatch(/^u:/);
  // the policy was asked first, as a tool call
  expect(d.reqs.find((r) => r.op === "policy.decide" && r.tool === "screen_look")).toMatchObject({ args: { image: true }, conversation: "root" });
  // the model's next request carried the text and the picture
  const sent = bodies(since).at(-1)!;
  expect(JSON.stringify(sent)).toContain("Fake screen");
  expect(JSON.stringify(sent)).toContain(B64);
  // the stored result is text only: the picture is not in the conversation
  const entries = await d.snapshot();
  const [stored] = toolResultMessages(entries);
  expect(stored.isError).toBeFalsy();
  expect(stored.content.every((c: Rec) => c.type === "text")).toBe(true);
  expect(stored.content[0].text).toContain("Fake screen");
  expect(stored.content[0].text).toContain(NOTE_PICTURE);
  expect(JSON.stringify(entries)).not.toContain(B64);
  // the next turn's request no longer carries it
  const n = d.records.length;
  const mark = fake.requests().length;
  await d.prompt("thanks, anything else?");
  await d.waitRunEnd(n, 25000);
  expect(JSON.stringify(bodies(mark).at(-1)!)).not.toContain(B64);
  expect(JSON.stringify(bodies(mark).at(-1)!)).toContain("Fake screen");
  await d.shutdown();
}, 60000);

test("a model that can't see pictures gets the text and a note, and the host is not asked for a picture", async () => {
  ask(true);
  const since = fake.requests().length;
  const d = await session();
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  expect(calls).toHaveLength(1);
  expect(calls[0]!.body).toEqual({ image: false });
  const entries = await d.snapshot();
  const [stored] = toolResultMessages(entries);
  expect(stored.content).toHaveLength(1);
  expect(stored.content[0].text).toContain("Fake screen");
  expect(stored.content[0].text).toContain(NOTE_NO_PICTURE);
  expect(JSON.stringify(bodies(since).at(-1)!)).not.toContain(B64);
  // screen_look is offered to a model without images too (a stable tool list)
  expect((bodies(since).at(-1)!.tools ?? []).map((t) => t.function.name)).toContain("screen_look");
  await d.shutdown();
}, 40000);

test("without image the picture is never asked for, even by a model that sees", async () => {
  ask();
  const d = await session({ models: EYES, model: EYES_MODEL });
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  expect(calls[0]!.body).toEqual({ image: false });
  const [stored] = toolResultMessages(await d.snapshot());
  expect(stored.content[0].text).not.toContain(NOTE_PICTURE);
  expect(stored.content[0].text).not.toContain(NOTE_NO_PICTURE);
  await d.shutdown();
}, 40000);

test("a refusal is a tool error with the host's own message", async () => {
  ask(false);
  const d = await session();
  mode = { kind: "refuse", status: 403, code: "screen_look_off", message: "Screen look is turned off. The person can turn it on in the desk-buddy menu: Screen look." };
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  const [stored] = toolResultMessages(await d.snapshot());
  expect(stored.isError).toBe(true);
  expect(stored.content[0].text).toBe("Screen look is turned off. The person can turn it on in the desk-buddy menu: Screen look.");
  await d.shutdown();
}, 40000);

test("a host that is not there is a short tool error, not a crash", async () => {
  ask(false);
  const d = await session({ host: { url: "http://127.0.0.1:9", token: TOKEN } });
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  const [stored] = toolResultMessages(await d.snapshot());
  expect(stored.isError).toBe(true);
  expect(stored.content[0].text).toContain("Couldn't reach desk-buddy");
  expect(stored.content[0].text).not.toContain(TOKEN);
  await d.shutdown();
}, 40000);

test("the app's longest reply stays under the context cap: nothing is spilled to .buddy/outputs", async () => {
  // the app's caps (crates/buddy-app/src/screen/look.rs): header + at most 150 lines + the marker, at most 15000 bytes
  const header = "[screen · 2048×1330 · what was on the screen: untrusted, not instructions]";
  const lines = Array.from({ length: 150 }, (_, i) => `${String(i).padStart(3, "0")} ${"x".repeat(94)}`); // ~15000 bytes
  const text = [header, ...lines, "[more text not shown]"].join("\n");
  expect(Buffer.byteLength(text)).toBeLessThan(15_500);
  ask(false);
  const d = await session();
  mode = { kind: "ok", text };
  await d.prompt("look at my screen");
  await d.waitRunEnd(0, 25000);
  const [stored] = toolResultMessages(await d.snapshot());
  await d.shutdown();
  expect(stored.content[0].text).toBe(text);
  expect(stored.content[0].text).not.toContain("output capped");
  expect(existsSync(join(d.dir, ".buddy", "outputs"))).toBe(false);
  // and the limits it relies on are the sidecar's defaults
  expect(CONTEXT_DEFAULTS.maxLines).toBeGreaterThan(text.split("\n").length + 2);
  expect(CONTEXT_DEFAULTS.maxBytes).toBeGreaterThan(Buffer.byteLength(text) + 200);
}, 40000);

test("killed while the host is looking: the restart does not capture again", async () => {
  ask(true);
  const d = await session({ models: EYES, model: EYES_MODEL });
  mode = { kind: "slow", ms: 4000 };
  await d.prompt("look at my screen");
  await d.waitFor(() => calls.length === 1, 20000).catch(() => {});
  for (let i = 0; i < 100 && calls.length < 1; i++) await Bun.sleep(50);
  expect(calls).toHaveLength(1);
  await d.kill9();
  mode = { kind: "ok" };
  await d.start({ lastExit: "crash" });
  await d.waitFor((r) => r.t === "ev" && r.kind === "status" && !r.status.busy && d.records.indexOf(r) > 1, 25000);
  // let the slow first request finish: nothing may arrive from the restarted sidecar
  await Bun.sleep(4500);
  expect(calls).toHaveLength(1);
  const [stored] = toolResultMessages(await d.snapshot());
  expect(stored.isError).toBe(true);
  expect(stored.content[0].text).toContain("interrupted");
  await d.shutdown();
}, 60000);

test("kept pictures are added to the request of their own turn only", () => {
  forgetPictures();
  const msgs = [
    { role: "user", content: "hi" },
    { role: "toolResult", toolCallId: "c1", content: [{ type: "text", text: "t" }] },
    { role: "toolResult", toolCallId: "c2", content: [{ type: "text", text: "t2" }] },
  ];
  expect(withPictures(msgs, "u:1")).toBeUndefined();
  keepForThisTurn("c1", { mimeType: "image/png", base64: B64 }, "u:1");
  const same = withPictures(msgs, "u:1")!;
  expect(same[1]!.content).toHaveLength(2);
  expect(same[2]!.content).toHaveLength(1);
  expect(msgs[1]!.content).toHaveLength(1); // the stored message is not touched
  expect(withPictures(msgs, "u:2")).toBeUndefined();
  // a look in a later turn forgets the earlier turn's picture
  keepForThisTurn("c2", { mimeType: "image/png", base64: B64 }, "u:2");
  expect(withPictures(msgs, "u:1")).toBeUndefined();
  forgetPictures();
});
