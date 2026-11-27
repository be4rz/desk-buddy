// memory.extract and set_memory on the scripted fake model: the extractor reads only the person's words and the bear's
// final answer, makes one direct model call outside the conversation, maps each item back to its turn, and never
// writes to the chat. set_memory swaps the "What I know about you" section for the next request.

import { afterAll, expect, test } from "bun:test";
import { Driver, lastAnswer, type Rec } from "../support/driver.ts";
import { type Reply, type RecordedRequest, startFake } from "../support/fake.ts";

const fake = startFake({ script: [] });
afterAll(() => fake.stop());

const SAIGON = "## What I know about you\n\nAbout them\n- Lives in Saigon";
const HANOI = "## What I know about you\n\nAbout them\n- Lives in Hanoi";
const REGISTRY = [
  { key: "home_city", kind: "profile", one: true, qualifier: null },
  { key: "people", kind: "profile", one: false, qualifier: "relation" },
];
const TOOL_OUT = "TOOLOUT_7731";

type Msg = { role: string; content: unknown };
const bodies = (since = 0) =>
  fake
    .requests()
    .slice(since)
    .filter((r: RecordedRequest) => r.path.endsWith("/chat/completions"))
    .map((r) => JSON.parse(r.body) as { messages: Msg[] });
const flat = (b: { messages: Msg[] }) => JSON.stringify(b.messages);
const system = (b: { messages: Msg[] }) => String(b.messages.find((m) => m.role === "system")?.content ?? "");
const items = (...list: object[]): Reply => ({ text: JSON.stringify({ items: list }) });
/** The extractor's own request: its user message opens with "Today is". */
const extractRequests = (since: number) => bodies(since).filter((b) => b.messages.some((m) => m.role === "user" && String(m.content).startsWith("Today is")));

/** The chat script: "Saigon" runs a shell command, and its result gets the final answer. */
const chat = (extract: Reply): Parameters<typeof fake.setScript>[0] => [
  { match: "Known facts:", reply: extract },
  { match: "Saigon", reply: { toolCalls: [{ name: "bash", args: { command: `echo ${TOOL_OUT}` } }] }, times: 1 },
  { match: TOOL_OUT, reply: { text: "Saigon is lovely, noted." } },
  { match: /./, reply: { text: "ok" } },
];

const args = (ids: string[], extra: Record<string, unknown> = {}) => ({
  turns: ids.map((requestId) => ({ requestId, endedAt: "2026-11-27T09:12:03+07:00" })),
  known: [{ id: 12, subject: "user", predicate: "home_city", qualifier: null, value: "Saigon" }],
  registry: REGISTRY,
  today: "2026-11-27",
  episodes: false,
  ...extra,
});

async function chatted(said = "I live in Saigon, please remember it.") {
  const d = new Driver({ fakeUrl: fake.url });
  expect((await d.start()).t).toBe("ready");
  const id = `u:${crypto.randomUUID()}`;
  await d.prompt(said, id);
  await d.waitRunEnd(0, 20000);
  return { d, id };
}

test("memory.extract: parsed items map back to the turn; turns carry the person's words; the model sees input and answer, not tool output", async () => {
  fake.setScript(chat(items({ exchange: 1, kind: "profile", predicate: "home_city", qualifier: null, value: "Saigon", text: "Lives in Saigon", importance: 8, valid_from: "2026-10", quote: "I live in Saigon" })));
  const { d, id } = await chatted();
  const since = fake.requests().length;
  const ack = await d.send("memory.extract", args([id]));
  expect(ack.ok).toBe(true);
  expect(ack.result).toEqual({
    items: [{ turn: id, kind: "profile", predicate: "home_city", qualifier: null, value: "Saigon", text: "Lives in Saigon", importance: 8, validFrom: "2026-10", quote: "I live in Saigon" }],
    turns: [{ requestId: id, said: "I live in Saigon, please remember it." }],
    missing: [],
  });
  const [req] = extractRequests(since);
  expect(extractRequests(since)).toHaveLength(1);
  const sent = flat(req);
  expect(sent).toContain("I live in Saigon, please remember it.");
  expect(sent).toContain("Saigon is lovely, noted.");
  expect(sent).toContain("home_city");
  expect(sent).not.toContain(TOOL_OUT);
  // The extractor's own prompt is not the bear's: no persona, no tools, no chat history.
  expect(system(req)).toContain("You maintain the long-term memory");
  expect(system(req)).not.toContain("desk-buddy");
  expect(system(req)).not.toContain("| episode");
  expect(req.messages.filter((m) => m.role === "user")).toHaveLength(1);
  await d.shutdown();
}, 40000);

test("memory.extract: an id with no input entry is missing, and nothing is added to the chat", async () => {
  fake.setScript(chat(items({ exchange: 1, kind: "preference", predicate: null, qualifier: null, value: null, text: "Likes short replies", importance: 5, quote: null })));
  const { d, id } = await chatted();
  const before = JSON.stringify(await d.snapshot());
  const live = d.entries().length;
  const ghost = `u:${crypto.randomUUID()}`;
  const ack = await d.send("memory.extract", args([ghost, id]));
  expect(ack.result.missing).toEqual([ghost]);
  expect(ack.result.turns.map((t: Rec) => t.requestId)).toEqual([id]);
  // The item's exchange 1 is the first turn that exists, not the first asked.
  expect(ack.result.items[0]).toMatchObject({ turn: id, kind: "preference", predicate: null, validFrom: null, quote: null });
  // Nothing is written to the conversation: same snapshot, no new entry or status, and no helper conversation's output.
  expect(JSON.stringify(await d.snapshot())).toBe(before);
  expect(d.entries().length).toBe(live);
  expect(lastAnswer(d.entries())).toBe("Saigon is lovely, noted.");

  // Every id missing: no model call.
  const since = fake.requests().length;
  const none = await d.send("memory.extract", args([ghost]));
  expect(none.result).toEqual({ items: [], turns: [], missing: [ghost] });
  expect(extractRequests(since)).toHaveLength(0);
  await d.shutdown();
}, 40000);

test("memory.extract: bad output (after one repair) is bad_output; a fenced or cut-off answer is repaired", async () => {
  fake.setScript(chat({ text: "I could not find anything worth keeping." }));
  const { d, id } = await chatted();
  expect(await d.send("memory.extract", args([id]))).toMatchObject({ ok: false, error: { code: "bad_output" } });
  fake.setScript(chat({ text: '{"result": []}' }));
  expect(await d.send("memory.extract", args([id]))).toMatchObject({ ok: false, error: { code: "bad_output" } });
  const item = { exchange: 1, kind: "profile", predicate: "home_city", value: "Saigon", text: "Lives in Saigon", importance: 8, quote: "I live in Saigon" };
  fake.setScript(chat({ text: `Here you go:\n\`\`\`json\n${JSON.stringify({ items: [item] })}\n\`\`\`` }));
  expect((await d.send("memory.extract", args([id]))).result.items).toHaveLength(1);
  // Cut off mid-way through the second item: the first whole one is kept.
  fake.setScript(chat({ text: `{"items": [${JSON.stringify(item)}, {"exchange": 1, "kind": "profi` }));
  expect((await d.send("memory.extract", args([id]))).result.items).toHaveLength(1);
  // An item naming an exchange that does not exist is dropped.
  fake.setScript(chat(items({ ...item, exchange: 4 })));
  expect((await d.send("memory.extract", args([id]))).result.items).toEqual([]);
  await d.shutdown();
}, 60000);

test("memory.extract: arguments are checked (1 to 10 u: turns); episodes:true keeps the episode kind in the prompt", async () => {
  fake.setScript(chat(items()));
  const { d, id } = await chatted();
  const refused = async (a: Record<string, unknown>) => expect(await d.send("memory.extract", a)).toMatchObject({ ok: false, error: { code: "bad_request" } });
  await refused({ ...args([id]), turns: [] });
  await refused(args(Array.from({ length: 11 }, () => `u:${crypto.randomUUID()}`)));
  await refused(args([`s:${crypto.randomUUID()}`]));
  await refused(args(["u:nope"]));
  await refused({ ...args([id]), today: "yesterday" });
  const since = fake.requests().length;
  expect((await d.send("memory.extract", args([id], { episodes: true }))).result).toMatchObject({ items: [] });
  expect(system(extractRequests(since)[0])).toContain("profile | preference | episode | promise");
  await d.shutdown();
}, 40000);

test("memory.extract: no model is no_provider", async () => {
  fake.setScript(chat(items()));
  const { d, id } = await chatted();
  expect((await d.send("set_model", { model: null })).ok).toBe(true);
  expect(await d.send("memory.extract", args([id]))).toMatchObject({ ok: false, error: { code: "no_provider" } });
  await d.shutdown();
}, 40000);

test("set_memory: Init.memory is in the first request; the next one carries the new text and not the old; null empties; over the cap is bad_request", async () => {
  fake.setScript([{ match: /./, reply: { text: "ok" } }]);
  const d = new Driver({ fakeUrl: fake.url, memory: SAIGON });
  expect((await d.start()).t).toBe("ready");
  let since = fake.requests().length;
  await d.prompt("hello");
  await d.waitRunEnd(0, 20000);
  const first = system(bodies(since).at(-1)!);
  expect(first).toContain("Lives in Saigon");
  expect(first.indexOf("What I know about you")).toBeGreaterThan(first.indexOf("desk-buddy"));
  // Last: nothing but the memory text follows the skills.
  expect(first.endsWith(SAIGON)).toBe(true);

  expect(await d.send("set_memory", { memory: HANOI })).toMatchObject({ ok: true, result: {} });
  since = fake.requests().length;
  let n = d.records.length;
  await d.prompt("hello again");
  await d.waitRunEnd(n, 20000);
  const second = system(bodies(since).at(-1)!);
  expect(second).toContain("Lives in Hanoi");
  expect(second).not.toContain("Saigon");

  expect(await d.send("set_memory", { memory: "x".repeat(4001) })).toMatchObject({ ok: false, error: { code: "bad_request" } });
  expect(await d.send("set_memory", { memory: 7 })).toMatchObject({ ok: false, error: { code: "bad_request" } });
  // A refused text changes nothing.
  since = fake.requests().length;
  n = d.records.length;
  await d.prompt("still here");
  await d.waitRunEnd(n, 20000);
  expect(system(bodies(since).at(-1)!)).toContain("Lives in Hanoi");

  expect((await d.send("set_memory", { memory: null })).ok).toBe(true);
  since = fake.requests().length;
  n = d.records.length;
  await d.prompt("and now");
  await d.waitRunEnd(n, 20000);
  const last = system(bodies(since).at(-1)!);
  expect(last).not.toContain("What I know about you");
  expect(last).not.toContain("Hanoi");
  await d.shutdown();
}, 60000);
