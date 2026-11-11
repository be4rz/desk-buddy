// Unit checks of the prefix sections: the app's "Your body" text goes right after the preamble.

import { expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { sections } from "../../src/ext/core/prompt.ts";
import type { Init } from "../../src/protocol.ts";

const BODY = "## Your body\n\nYou are the bear on the user's screen.\n\nEmotes (`buddy mascot emote <name>`; a short gesture during your reply):\n- wave: a happy little wave";

function init(body?: string): Init {
  const skillsDir = mkdtempSync(join(tmpdir(), "buddy-prompt-"));
  return { platform: "macos", shellPath: "/bin/bash", skillsDir, ...(body === undefined ? {} : { body }) } as unknown as Init;
}

const render = (s: ReturnType<typeof sections>[number]) => (s.render as () => string | undefined)();

test("body: a section right after the preamble, untagged, the app's text verbatim", () => {
  const out = sections(init(BODY));
  expect(out.map((s) => s.key)).toEqual(["preamble", "body", "skills"]);
  expect(out[1].tag).toBe(false);
  expect(render(out[1])).toBe(BODY);
});

test("body: absent or blank adds no section", () => {
  expect(sections(init()).map((s) => s.key)).toEqual(["preamble", "skills"]);
  expect(sections(init("  \n")).map((s) => s.key)).toEqual(["preamble", "skills"]);
});

test("preamble: speaking with buddy say, moving in the body section, no static emote list", () => {
  const text = render(sections(init())[0]) ?? "";
  expect(text).toContain('use `buddy say "<text>"` to speak; see "Your body" for moving.');
  expect(text).not.toContain("mascot emote");
});
