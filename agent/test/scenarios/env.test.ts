// S2 criterion 3 (sidecar half): a command runs with exactly Rust's shell env
// plus BUDDY_API, BUDDY_TOKEN and BUDDY_TURN = the request id of the turn
// that ran it. No credential and nothing of the sidecar's own env leaks in.

import { afterAll, expect, test } from "bun:test";
import { Driver, toolResults } from "../support/driver.ts";
import { startFake } from "../support/fake.ts";

const fake = startFake({ script: [{ match: "show env", reply: { toolCalls: [{ name: "bash", args: { command: "env | sort" } }] } }] });
afterAll(() => fake.stop());

test("BUDDY_TURN is the prompt's request id; the env is exact", async () => {
  const d = new Driver({
    fakeUrl: fake.url,
    host: { url: "http://127.0.0.1:4555", token: "agent-token-test" },
    shellEnv: { PATH: "/usr/bin:/bin", HOME: "/tmp/buddy-home", LANG: "en_US.UTF-8" },
  });
  await d.start();
  const first = `u:${crypto.randomUUID()}`;
  await d.prompt("show env", first);
  await d.waitRunEnd();
  const second = `u:${crypto.randomUUID()}`;
  const mark = d.records.length;
  await d.prompt("show env", second);
  await d.waitRunEnd(mark);
  const [a, b] = toolResults(await d.snapshot());
  await d.shutdown();
  expect(a.text).toContain(`BUDDY_TURN=${first}`);
  expect(b.text).toContain(`BUDDY_TURN=${second}`);
  expect(a.text).toContain("BUDDY_API=http://127.0.0.1:4555");
  expect(a.text).toContain("BUDDY_TOKEN=agent-token-test");
  expect(a.text).toContain("HOME=/tmp/buddy-home");
  // Nothing inherited: the sidecar's TMPDIR is not in the shell env, and no key is.
  expect(a.text).not.toContain("TMPDIR=");
  expect(a.text).not.toContain("sk-fake-credential-7c1f");
}, 30000);

test("without a host API the BUDDY_API and BUDDY_TOKEN variables are absent", async () => {
  const d = new Driver({ fakeUrl: fake.url });
  await d.start();
  await d.prompt("show env");
  await d.waitRunEnd();
  const [a] = toolResults(await d.snapshot());
  await d.shutdown();
  expect(a.text).not.toContain("BUDDY_API=");
  expect(a.text).not.toContain("BUDDY_TOKEN=");
  expect(a.text).toContain("BUDDY_TURN=u:");
}, 30000);
