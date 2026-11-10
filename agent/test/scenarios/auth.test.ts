// accounts (AC8), sidecar half: `auth.login` drives pi-ai's sign-in through
// `req auth.*` and writes the credential with `cred.modify`; a wrong code and a
// cancel fail cleanly; the curated provider list matches pi-ai; `s:` ids are
// sense turns.

import { afterAll, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { builtinModels } from "@earendil-works/pi-ai/providers/all";
import { requestSource } from "../../src/protocol.ts";
import { Driver, type Rec } from "../support/driver.ts";
import { startFake } from "../support/fake.ts";

const fake = startFake({ script: [{ match: "hello", reply: { text: "Signed-in hello." } }] });
afterAll(() => fake.stop());
const FAKE_OAUTH = { fakeOAuth: { baseUrl: `${fake.url}/v1` } };

const ops = (d: Driver) => d.reqs.map((r) => r.op);

test("OAuth sign-in: open_url → prompt → cred.modify, then the model works with it", async () => {
  const opened: Rec[] = [];
  const d = new Driver({
    fakeUrl: fake.url,
    credentials: {},
    model: null,
    settings: FAKE_OAUTH,
    handlers: {
      auth: (r) => {
        if (r.op === "auth.open_url") opened.push(r);
        return r.op === "auth.prompt" ? { value: "good" } : {};
      },
    },
  });
  await d.start();
  const ack = await d.send("auth.login", { providerId: "fake-oauth" });
  expect(ack).toMatchObject({ ok: true, result: { providerId: "fake-oauth", ok: true } });
  expect(opened[0]).toMatchObject({ url: "http://127.0.0.1:1/fake-oauth/authorize", instructions: "Paste the code the page shows." });
  const seq = ops(d).filter((o) => o.startsWith("auth.") || o.startsWith("cred.modify"));
  expect(seq.slice(0, 5)).toEqual(["auth.open_url", "auth.prompt", "auth.progress", "cred.modify_begin", "cred.modify_end"]);
  expect(d.credentials.get("fake-oauth")).toMatchObject({ type: "oauth", access: "fake-access-1" });

  // The new credential serves the model.
  await d.send("set_model", { model: { provider: "fake-oauth", model: "fake-1" } });
  const before = fake.requests().length;
  await d.prompt("hello");
  await d.waitRunEnd();
  const sent = fake.requests().slice(before).find((r) => r.path.endsWith("/chat/completions"));
  expect(sent?.headers.authorization).toBe("Bearer fake-access-1");
  // The token travels only in cred.modify_end, to Rust: in no other record, nor in stderr.
  const others = d.records.filter((r) => !(r.t === "req" && String(r.op).startsWith("cred.")));
  expect(JSON.stringify(others)).not.toContain("fake-refresh-1");
  expect(JSON.stringify(others)).not.toContain("fake-access-1");
  expect(d.stderr).not.toContain("fake-access-1");
  await d.shutdown();
}, 30000);

test("a wrong code fails the sign-in with pi-ai's message and writes nothing", async () => {
  const d = new Driver({ fakeUrl: fake.url, credentials: {}, model: null, settings: FAKE_OAUTH, handlers: { auth: (r) => (r.op === "auth.prompt" ? { value: "abc123#state" } : {}) } });
  await d.start();
  const ack = await d.send("auth.login", { providerId: "fake-oauth" });
  expect(ack.ok).toBe(false);
  expect(ack.error).toMatchObject({ code: "auth" });
  expect(ack.error.message).toContain("invalid_grant");
  expect(ops(d)).not.toContain("cred.modify_begin");
  expect(d.credentials.has("fake-oauth")).toBe(false);
  await d.shutdown();
}, 30000);

test("auth.cancel ends a sign-in waiting on its code; the window's prompt is cancelled", async () => {
  const d = new Driver({ fakeUrl: fake.url, credentials: {}, model: null, settings: FAKE_OAUTH, handlers: { auth: (r) => (r.op === "auth.prompt" ? new Promise(() => {}) : {}) } });
  await d.start();
  const login = d.send("auth.login", { providerId: "fake-oauth" });
  await d.waitFor((r) => r.t === "req" && r.op === "auth.prompt");
  expect((await d.send("auth.cancel")).ok).toBe(true);
  const ack = await login;
  expect(ack.error).toMatchObject({ code: "cancelled" });
  const prompt = d.reqs.find((r) => r.op === "auth.prompt")!;
  await d.waitFor((r) => r.t === "cancel" && r.id === prompt.id);
  await d.shutdown();
}, 30000);

test("the window's Cancel answering the prompt first still ends the sign-in as cancelled", async () => {
  const d = new Driver({ fakeUrl: fake.url, credentials: {}, model: null, settings: FAKE_OAUTH, handlers: { auth: (r) => (r.op === "auth.prompt" ? Promise.reject({ code: "cancelled", message: "Stopped." }) : {}) } });
  await d.start();
  const ack = await d.send("auth.login", { providerId: "fake-oauth" });
  expect(ack.error).toMatchObject({ code: "cancelled" });
  await d.shutdown();
}, 30000);

test("a sign-in page Rust refuses ends the sign-in with its reason, without waiting on a code", async () => {
  const d = new Driver({
    fakeUrl: fake.url,
    credentials: {},
    model: null,
    settings: FAKE_OAUTH,
    handlers: {
      auth: (r) => {
        if (r.op === "auth.open_url") return Promise.reject({ code: "auth", message: "That sign-in page isn't an https address." });
        return r.op === "auth.prompt" ? new Promise(() => {}) : {};
      },
    },
  });
  await d.start();
  const ack = await d.send("auth.login", { providerId: "fake-oauth" });
  expect(ack.error).toMatchObject({ code: "auth", message: "That sign-in page isn't an https address." });
  expect(d.credentials.has("fake-oauth")).toBe(false);
  await d.shutdown();
}, 30000);

test("without the test hook there is no fake-oauth provider, and auth.login needs a provider", async () => {
  const d = new Driver({ fakeUrl: fake.url, credentials: {}, model: null });
  await d.start();
  expect((await d.send("auth.login", { providerId: "fake-oauth" })).error.message).toContain("Unknown provider");
  expect((await d.send("auth.login", {})).error).toMatchObject({ code: "bad_request" });
  await d.shutdown();
}, 30000);

test("a sense turn: s:<uuid> is accepted with source sense; s:nope is refused", async () => {
  expect(requestSource(`s:${crypto.randomUUID()}`)).toBe("sense");
  expect(requestSource("s:nope")).toBeUndefined();
  const d = new Driver({ fakeUrl: fake.url });
  await d.start();
  const requestId = `s:${crypto.randomUUID()}`;
  expect(await d.send("prompt", { requestId, text: "hello (back from idle, morning)", source: "sense" })).toMatchObject({ ok: true });
  await d.waitRunEnd();
  expect(d.entries().find((e) => e.kind === "buddy.input")?.data).toMatchObject({ requestId, source: "sense" });
  expect((await d.send("prompt", { requestId: "s:nope", text: "x" })).error).toMatchObject({ code: "bad_request" });
  await d.shutdown();
}, 30000);

test("the Sign in… window's providers exist in pi-ai with their methods and default model", () => {
  const list = JSON.parse(readFileSync(join(import.meta.dir, "..", "..", "..", "crates", "buddy-app", "src", "signin", "providers.json"), "utf8")) as { id: string; methods: string[]; model: string }[];
  const models = builtinModels({ credentials: {} as any });
  expect(list.length).toBeGreaterThan(0);
  for (const p of list) {
    const prov = models.getProvider(p.id);
    expect(prov, p.id).toBeDefined();
    expect(models.getModels(p.id).some((m) => m.id === p.model), `${p.id}/${p.model}`).toBe(true);
    for (const m of p.methods) {
      expect(["oauth", "api_key"]).toContain(m);
      if (m === "oauth") expect(prov!.auth.oauth, `${p.id} oauth`).toBeDefined();
      if (m === "api_key") expect(prov!.auth.apiKey?.login, `${p.id} api key`).toBeDefined();
    }
  }
});
