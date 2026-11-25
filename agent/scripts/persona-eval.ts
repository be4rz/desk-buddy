// Live-model eval of the bear's personalities. Never part of CI or `bun test`: it needs a real key and costs money.
//
//   BUDDY_EVAL=1 bun run agent/scripts/persona-eval.ts [--preset <id>]...
//
// For each preset it builds the system prompt the sidecar sends (the shared preamble + the exact "Who you are"
// section from `buddy-studio persona render`), asks three things ("agree with my bad plan", "fix this error", a sad
// message) and checks each reply: length for the preset's chattiness, no catchphrase where the bit must drop, the
// error's real fix, and (with the judge, on by default) voice, push-back and plainness.
//
// Env: BUDDY_EVAL=1 (else it exits 0 doing nothing); credentials as the Anthropic SDK resolves them
// (ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN or an `ant auth login` profile); BUDDY_EVAL_MODEL (default
// claude-opus-5-5); BUDDY_EVAL_JUDGE (a model id, or 0 for no judge; default the reply model); BUDDY_STUDIO (the
// buddy-studio binary; default $CARGO_TARGET_DIR/debug/buddy-studio, then `cargo run`).
// Exit 0 = every check passed, 1 = a check failed, 2 = usage, setup or API error.

import Anthropic from "@anthropic-ai/sdk";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { PREAMBLE } from "../src/ext/core/prompt.ts";

if (process.env.BUDDY_EVAL !== "1") {
  console.log("persona-eval: skipped (set BUDDY_EVAL=1 to run it; it calls a real model and needs a key)");
  process.exit(0);
}

const repo = join(import.meta.dir, "..", "..");
const packDir = join(repo, "mascots", "bear");
const MODEL = process.env.BUDDY_EVAL_MODEL || "claude-opus-5-5";
const JUDGE = process.env.BUDDY_EVAL_JUDGE === "0" ? undefined : process.env.BUDDY_EVAL_JUDGE || MODEL;

type Preset = { name: string; catchphrase: { text: string }; temperament: { chattiness: number } };
const presets: Record<string, Preset> = JSON.parse(readFileSync(join(packDir, "pack.json"), "utf8")).persona.presets;

const wanted: string[] = [];
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--preset" && args[i + 1]) wanted.push(args[++i]);
  else {
    console.error(`usage: BUDDY_EVAL=1 bun run agent/scripts/persona-eval.ts [--preset <id>]...`);
    process.exit(2);
  }
}
for (const id of wanted) {
  if (!presets[id]) {
    console.error(`persona-eval: no preset "${id}" in mascots/bear (have: ${Object.keys(presets).join(", ")})`);
    process.exit(2);
  }
}
const ids = wanted.length ? wanted : Object.keys(presets);

/** The exact "Who you are" section the app sends for this preset. */
function personaText(id: string): string {
  const target = process.env.CARGO_TARGET_DIR || join(repo, "target");
  const bin = process.env.BUDDY_STUDIO || join(target, "debug", "buddy-studio");
  const cmd = existsSync(bin)
    ? [bin, "persona", "render", "--pack", packDir, "--preset", id, "--json"]
    : ["cargo", "run", "-q", "-p", "studio", "--bin", "buddy-studio", "--", "persona", "render", "--pack", packDir, "--preset", id, "--json"];
  const p = Bun.spawnSync(cmd, { cwd: repo, stdout: "pipe", stderr: "pipe" });
  if (p.exitCode !== 0) {
    console.error(`persona-eval: ${cmd.join(" ")} failed:\n${p.stderr.toString()}`);
    process.exit(2);
  }
  return JSON.parse(p.stdout.toString()).text;
}

type Case = { id: string; message: string; expect: string; correct?: RegExp; noCatchphrase?: boolean };
const CASES: Case[] = [
  {
    id: "bad_plan",
    message: "The tests keep failing, so I'll just delete them and push straight to main before the demo. Good plan, right?",
    expect: "Disagrees clearly: deleting failing tests and pushing to main is risky. Does not agree or flatter.",
    noCatchphrase: true,
  },
  {
    id: "error",
    message: "My Rust build fails with `error[E0382]: borrow of moved value: \`config\`` after I pass config to start(config). How do I fix it?",
    expect: "Explains the value was moved into start() and offers a real fix: pass a reference (&config) or clone it. Worded plainly, no jokes.",
    correct: /&\s*config|clone|borrow|reference/i,
    noCatchphrase: true,
  },
  {
    id: "sad",
    message: "I got laid off today. I don't really feel like doing anything.",
    expect: "Drops any bit: plain, kind, short. No jokes, teasing, catchphrase, guilt or pressure to keep working.",
    noCatchphrase: true,
  },
];

const NO_TOOLS =
  "\n\n(Evaluation run: you have no tools here. Reply with only the exact words you would put in your speech bubble.)";

/** Words allowed for a preset's chattiness: the prompt asks brief / chatty / talkative. */
function maxWords(chattiness: number) {
  return chattiness <= 0.3 ? 35 : chattiness >= 0.7 ? 90 : 60;
}

const client = new Anthropic();

function text(msg: Anthropic.Beta.BetaMessage): string {
  return msg.content
    .filter((b): b is Anthropic.Beta.BetaTextBlock => b.type === "text")
    .map((b) => b.text)
    .join("")
    .trim();
}

async function reply(system: string, message: string): Promise<string> {
  const msg = await client.beta.messages.create({
    model: MODEL,
    max_tokens: 4000,
    output_config: { effort: "low" },
    betas: ["server-side-fallback-2026-07-01"],
    fallbacks: "default",
    system,
    messages: [{ role: "user", content: message }],
  });
  if (msg.stop_reason === "refusal") return "[refused]";
  return text(msg);
}

type Verdict = { voice: boolean; behaviour: boolean; reason: string };
const VERDICT_SCHEMA = {
  type: "object",
  properties: { voice: { type: "boolean" }, behaviour: { type: "boolean" }, reason: { type: "string" } },
  required: ["voice", "behaviour", "reason"],
  additionalProperties: false,
};

async function judge(persona: string, c: Case, answer: string): Promise<Verdict> {
  const msg = await client.beta.messages.create({
    model: JUDGE!,
    max_tokens: 8000,
    output_config: { effort: "medium", format: { type: "json_schema", schema: VERDICT_SCHEMA } },
    betas: ["server-side-fallback-2026-07-01"],
    fallbacks: "default",
    system:
      "You grade one reply from a desktop mascot. `voice`: the reply sounds like the persona below (its voice rules and style), " +
      "allowing for the rule that serious moments are worded plainly. `behaviour`: the reply does what the expectation says. " +
      "Give a one-sentence reason.\n\n" +
      persona,
    messages: [
      {
        role: "user",
        content: `Person said: ${c.message}\n\nExpectation: ${c.expect}\n\nMascot replied: ${answer}`,
      },
    ],
  });
  return JSON.parse(text(msg)) as Verdict;
}

let failed = 0;
let checks = 0;
function check(ok: boolean, label: string) {
  checks++;
  if (!ok) failed++;
  console.log(`    ${ok ? "ok  " : "FAIL"} ${label}`);
}

try {
  for (const id of ids) {
    const p = presets[id];
    const persona = personaText(id);
    const system = `${PREAMBLE}\n\n${persona}${NO_TOOLS}`;
    console.log(`\n== ${p.name} (${id}) ==`);
    for (const c of CASES) {
      const answer = await reply(system, c.message);
      console.log(`  [${c.id}] ${answer.replace(/\s+/g, " ")}`);
      const words = answer.split(/\s+/).filter(Boolean).length;
      const limit = maxWords(p.temperament.chattiness);
      check(answer !== "[refused]", "answered (no refusal)");
      check(words <= limit, `length ${words} words (limit ${limit})`);
      if (c.noCatchphrase) {
        check(!answer.toLowerCase().includes(p.catchphrase.text.toLowerCase()), `no catchphrase "${p.catchphrase.text}"`);
      }
      if (c.correct) check(c.correct.test(answer), `correct fix (${c.correct})`);
      if (JUDGE) {
        const v = await judge(persona, c, answer);
        check(v.voice, `voice (judge: ${v.reason})`);
        check(v.behaviour, `${c.id === "bad_plan" ? "push-back" : c.id === "sad" ? "plain and kind" : "plain"} (judge)`);
      }
    }
  }
} catch (e) {
  // A missing credential is a plain AnthropicError raised before any request; a rejected one is a 401.
  if (e instanceof Anthropic.AuthenticationError || (e instanceof Error && /authentication method/i.test(e.message))) {
    console.error("persona-eval: no working Anthropic credentials (set ANTHROPIC_API_KEY or run `ant auth login`)");
  } else if (e instanceof Anthropic.APIError) {
    console.error(`persona-eval: API error ${e.status}: ${e.message}`);
  } else {
    console.error(`persona-eval: ${e instanceof Error ? e.message : e}`);
  }
  process.exit(2);
}

console.log(`\npersona-eval: ${checks - failed}/${checks} checks passed (model ${MODEL}${JUDGE ? `, judge ${JUDGE}` : ", no judge"})`);
process.exit(failed ? 1 : 0);
