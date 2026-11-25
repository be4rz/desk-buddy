// The Personality page's fill (crates/buddy-app/web/persona/fill.js) against the shared fixture the
// Rust `persona::fill` test runs too, so the sample bubble and the bear's own lines agree.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

type Fill = { me: string; you: string | null; catchphrase: string | null };
type Case = { name: string; text: string; me: string; you: string | null; catchphrase: string | null; out: string };

const repo = join(import.meta.dir, "..", "..", "..");
const src = readFileSync(join(repo, "crates/buddy-app/web/persona/fill.js"), "utf8");
const mod: { exports: { fill?: (text: string, f: Fill) => string } } = { exports: {} };
new Function("module", src)(mod);
const fill = mod.exports.fill!;
const fixture: { cases: Case[] } = JSON.parse(readFileSync(join(repo, "crates/mascot-core/tests/fixtures/persona_fill.json"), "utf8"));

test("the fixture has cases", () => {
  expect(typeof fill).toBe("function");
  expect(fixture.cases.length).toBeGreaterThan(10);
});

for (const c of fixture.cases) {
  test(`fill: ${c.name}`, () => {
    expect(fill(c.text, { me: c.me, you: c.you, catchphrase: c.catchphrase })).toBe(c.out);
  });
}
