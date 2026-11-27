// Live-model eval of the memory extractor. Never part of CI or `bun test`: it needs a real key and costs money.
//
//   BUDDY_EVAL=1 bun run agent/scripts/memory-eval.ts [--only <conversation-id>]...
//
// Runs about 20 invented conversations (agent/scripts/fixtures/memory-eval.json) through the exact prompt the sidecar
// sends (agent/src/ext/memory/prompt.ts), one batch per conversation, and compares the items with the expected ones.
// Reports: extraction precision and recall, the stale-change rate (a fact the person changed that the extractor
// missed), the invented-quote rate (a quote that is not in the person's words, as Rust's check would see it), items
// per exchange and tokens per batch. Run it whenever the extract prompt or the registry changes.
//
// Env: BUDDY_EVAL=1 (else it exits 0 doing nothing); credentials as the Anthropic SDK resolves them
// (ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN or an `ant auth login` profile); BUDDY_EVAL_MODEL (default claude-opus-5-5).
// Exit 0 = every threshold held, 1 = a threshold failed, 2 = usage, setup or API error.

import Anthropic from "@anthropic-ai/sdk";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseItems, toItems } from "../src/ext/memory/extract.ts";
import { type Exchange, extractSystemPrompt, extractUserPrompt } from "../src/ext/memory/prompt.ts";
import type { MemoryItem, MemoryKnown, MemoryRegistryEntry } from "../src/protocol.ts";

if (process.env.BUDDY_EVAL !== "1") {
  console.log("memory-eval: skipped (set BUDDY_EVAL=1 to run it; it calls a real model and needs a key)");
  process.exit(0);
}

const MODEL = process.env.BUDDY_EVAL_MODEL || "claude-opus-5-5";

type Expected = { kind: string; predicate: string | null; qualifier: string | null; value: string | null; keywords?: string[] };
type Conversation = {
  id: string;
  exchanges: { endedAt: string; said: string; answer: string }[];
  known: MemoryKnown[];
  expected: Expected[];
  changes?: string[];
};
type Fixture = { today: string; registry: MemoryRegistryEntry[]; conversations: Conversation[] };

const fixture: Fixture = JSON.parse(readFileSync(join(import.meta.dir, "fixtures", "memory-eval.json"), "utf8"));

const only: string[] = [];
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--only" && argv[i + 1]) only.push(argv[++i]);
  else {
    console.error("usage: BUDDY_EVAL=1 bun run agent/scripts/memory-eval.ts [--only <conversation-id>]...");
    process.exit(2);
  }
}
for (const id of only) {
  if (!fixture.conversations.some((c) => c.id === id)) {
    console.error(`memory-eval: no conversation "${id}" (have: ${fixture.conversations.map((c) => c.id).join(", ")})`);
    process.exit(2);
  }
}
const conversations = only.length ? fixture.conversations.filter((c) => only.includes(c.id)) : fixture.conversations;

/** Case-fold, NFC, collapse whitespace: the quote check's normal form (the Rust check is the authority). */
const norm = (s: string) => s.normalize("NFC").toLowerCase().replace(/\s+/g, " ").trim();

function matches(item: MemoryItem, e: Expected): boolean {
  if (item.kind !== e.kind) return false;
  if (e.predicate) {
    if (item.predicate !== e.predicate) return false;
    if (e.qualifier && norm(item.qualifier ?? "") !== norm(e.qualifier)) return false;
    return !e.value || norm(item.value ?? "").includes(norm(e.value));
  }
  return !item.predicate && (e.keywords ?? []).every((w) => norm(item.text).includes(norm(w)));
}

const client = new Anthropic();

async function extract(c: Conversation): Promise<{ items: MemoryItem[]; tokens: number; parsed: boolean }> {
  const exchanges: Exchange[] = c.exchanges.map((x, i) => ({ n: i + 1, ...x }));
  const msg = await client.beta.messages.create({
    model: MODEL,
    max_tokens: 4000,
    output_config: { effort: "low" },
    betas: ["server-side-fallback-2026-07-01"],
    fallbacks: "default",
    system: extractSystemPrompt(fixture.registry, false),
    messages: [{ role: "user", content: extractUserPrompt(fixture.today, c.known, exchanges) }],
  });
  const text = msg.content
    .filter((b): b is Anthropic.Beta.BetaTextBlock => b.type === "text")
    .map((b) => b.text)
    .join("");
  const raw = parseItems(text);
  // `turns` is only used for its length here: exchange N → the Nth entry.
  const items = raw ? toItems(raw, c.exchanges.map((_, i) => ({ requestId: String(i + 1) }))) : [];
  return { items, tokens: msg.usage.input_tokens + msg.usage.output_tokens, parsed: !!raw };
}

let returned = 0;
let correct = 0;
let expectedTotal = 0;
let found = 0;
let changed = 0;
let stale = 0;
let quoted = 0;
let invented = 0;
let unparsed = 0;
let exchangeTotal = 0;
let tokens = 0;

try {
  for (const c of conversations) {
    const r = await extract(c);
    tokens += r.tokens;
    exchangeTotal += c.exchanges.length;
    if (!r.parsed) unparsed++;
    const claimed = new Set<number>();
    for (const e of c.expected) {
      expectedTotal++;
      const at = r.items.findIndex((it, j) => !claimed.has(j) && matches(it, e));
      if (at !== -1) {
        claimed.add(at);
        found++;
        correct++;
      }
      if (c.changes?.includes(e.predicate ?? "")) {
        changed++;
        if (at === -1) stale++;
      }
    }
    returned += r.items.length;
    for (const it of r.items) {
      if (!it.quote) continue;
      quoted++;
      const said = norm(c.exchanges[Number(it.turn) - 1]?.said ?? "");
      if (it.quote.trim().length < 3 || !said.includes(norm(it.quote))) invented++;
    }
    const missed = c.expected.filter((e) => !r.items.some((it) => matches(it, e))).length;
    const extra = r.items.length - claimed.size;
    console.log(`  [${c.id}] ${r.parsed ? "" : "UNPARSED "}${r.items.length} items, ${claimed.size}/${c.expected.length} expected${missed || extra ? ` (missed ${missed}, extra ${extra})` : ""}, ${r.tokens} tokens`);
  }
} catch (e) {
  if (e instanceof Anthropic.AuthenticationError || (e instanceof Error && /authentication method/i.test(e.message))) {
    console.error("memory-eval: no working Anthropic credentials (set ANTHROPIC_API_KEY or run `ant auth login`)");
  } else if (e instanceof Anthropic.APIError) {
    console.error(`memory-eval: API error ${e.status}: ${e.message}`);
  } else {
    console.error(`memory-eval: ${e instanceof Error ? e.message : e}`);
  }
  process.exit(2);
}

const pct = (a: number, b: number) => (b ? `${((100 * a) / b).toFixed(0)}%` : "n/a");
const precision = returned ? correct / returned : 1;
const recall = expectedTotal ? found / expectedTotal : 1;
const staleRate = changed ? stale / changed : 0;
const inventedRate = quoted ? invented / quoted : 0;
console.log(`\nbatches: ${conversations.length} (${exchangeTotal} exchanges), unparsed answers: ${unparsed}`);
console.log(`precision:          ${pct(correct, returned)} (${correct}/${returned} returned items were expected)`);
console.log(`recall:             ${pct(found, expectedTotal)} (${found}/${expectedTotal} expected items found)`);
console.log(`stale-change rate:  ${pct(stale, changed)} (${stale}/${changed} changed facts left stale)`);
console.log(`invented quotes:    ${pct(invented, quoted)} (${invented}/${quoted} quotes not in the person's words)`);
console.log(`items per exchange: ${(returned / Math.max(exchangeTotal, 1)).toFixed(2)}`);
console.log(`tokens per batch:   ${Math.round(tokens / Math.max(conversations.length, 1))} (model ${MODEL})`);

// Thresholds: what a prompt change must not regress below. Judged by the human at handover as well.
const failures = [
  [precision >= 0.7, "precision >= 70%"],
  [recall >= 0.7, "recall >= 70%"],
  [staleRate <= 0.2, "stale-change rate <= 20%"],
  [inventedRate <= 0.1, "invented-quote rate <= 10%"],
  [unparsed === 0, "every answer parsed"],
].filter(([ok]) => !ok);
for (const [, label] of failures) console.log(`FAIL ${label}`);
process.exit(failures.length ? 1 : 0);
