// Persona section, on the scripted fake model: Init.persona reaches the first
// request, and after `set_persona` the next request carries the new text and not the old one (the section is
// replaced in the prefix, not appended).

import { afterAll, expect, test } from "bun:test";
import { Driver } from "../support/driver.ts";
import { type RecordedRequest, startFake } from "../support/fake.ts";

const fake = startFake({ script: [] });
afterAll(() => fake.stop());

const BISCUIT = "## Who you are\n\nYour name is Biscuit, a cheeky sidekick.\n\nHow you sound:\n- Dry and short";
const MOCHI = "## Who you are\n\nYour name is Mochi, a gentle companion.\n\nHow you sound:\n- Soft and warm";

type Body = { messages: { role: string; content: unknown }[] };
const bodies = (since = 0): Body[] =>
  fake
    .requests()
    .slice(since)
    .filter((r: RecordedRequest) => r.path.endsWith("/chat/completions"))
    .map((r) => JSON.parse(r.body));
const text = (b: Body) => JSON.stringify(b.messages);

async function session(persona?: string) {
  const d = new Driver({ fakeUrl: fake.url, ...(persona === undefined ? {} : { persona }) });
  expect((await d.start()).t).toBe("ready");
  return d;
}

test("Init.persona is in the first request; set_persona replaces it for the next one", async () => {
  fake.setScript([{ match: /./, reply: { text: "ok" } }]);
  const since = fake.requests().length;
  const d = await session(BISCUIT);
  await d.prompt("hello");
  await d.waitRunEnd(0, 20000);
  expect(text(bodies(since).at(-1)!)).toContain("Biscuit, a cheeky sidekick");

  expect((await d.send("set_persona", { persona: MOCHI })).ok).toBe(true);
  const n = d.records.length;
  const mark = fake.requests().length;
  await d.prompt("hello again");
  await d.waitRunEnd(n, 20000);
  await d.shutdown();
  const next = text(bodies(mark).at(-1)!);
  expect(next).toContain("Mochi, a gentle companion");
  expect(next).not.toContain("Biscuit");
  expect(next).toContain("hello again");
}, 40000);

test("set_persona with null removes the section; a non-string or oversized text is refused", async () => {
  fake.setScript([{ match: /./, reply: { text: "ok" } }]);
  const d = await session(BISCUIT);
  const bad = await d.send("set_persona", { persona: 7 });
  expect(bad.ok).toBe(false);
  const huge = await d.send("set_persona", { persona: "x".repeat(20001) });
  expect(huge.ok).toBe(false);
  expect((await d.send("set_persona", { persona: null })).ok).toBe(true);
  const mark = fake.requests().length;
  await d.prompt("hello");
  await d.waitRunEnd(0, 20000);
  await d.shutdown();
  const sent = text(bodies(mark).at(-1)!);
  expect(sent).not.toContain("Biscuit");
  expect(sent).toContain("small animated mascot");
}, 40000);
