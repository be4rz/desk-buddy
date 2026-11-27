// `memory.extract` (design I1): one direct model call that reads the person's words and the bear's final answer of
// up to 10 chat turns and returns the facts worth keeping. Rust owns the store and every rule; this only makes the
// call. Nothing is written to agent.sqlite and no entry reaches the chat: the transcript is read, never added to.
// Nothing here logs or returns text beyond the ack: `said`, quotes and item text stay out of every log line.

import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Conversation } from "@earendil-works/pi-durable";
import { currentModelCall } from "../../current.ts";
import { CmdError, type MemoryExtractAck, type MemoryItem, type MemoryKnown, type MemoryRegistryEntry, type MemoryTurnIn } from "../../protocol.ts";
import { messageText, type TextMessage } from "../../text.ts";
import { INPUT_KIND } from "../core/index.ts";
import { type Exchange, extractSystemPrompt, extractUserPrompt } from "./prompt.ts";

const ctx = BACKGROUND_CONTEXT;

export const MAX_TURNS = 10;
/** The person's words per turn, in characters. */
export const SAID_MAX = 4000;
/** The bear's final answer per turn. */
export const ANSWER_MAX = 2000;
export const EXTRACT_TIMEOUT_MS = 60_000;
const MAX_KNOWN = 200;
const MAX_ITEMS = 30;
const PAGE = 200;
const MAX_PAGES = 100;
const CHAT_ID = /^u:[0-9a-f-]{36}$/;
const DAY = /^\d{4}-\d{2}-\d{2}$/;

export type ExtractArgs = { turns: MemoryTurnIn[]; known: MemoryKnown[]; registry: MemoryRegistryEntry[]; today: string; episodes: boolean };

const bad = (message: string) => new CmdError("bad_request", message);
const clip = (s: string, max: number) => (s.length <= max ? s : [...s].slice(0, max).join(""));
const str = (v: unknown): string | null => (typeof v === "string" ? v : typeof v === "number" && Number.isFinite(v) ? String(v) : null);

/** Check the command's arguments (1..10 turns, `u:` ids only). */
export function parseArgs(args: Record<string, unknown>): ExtractArgs {
  const rawTurns = args.turns;
  if (!Array.isArray(rawTurns) || rawTurns.length < 1 || rawTurns.length > MAX_TURNS) throw bad(`memory.extract needs 1 to ${MAX_TURNS} turns.`);
  const turns: MemoryTurnIn[] = [];
  for (const t of rawTurns as Record<string, unknown>[]) {
    const requestId = t && typeof t === "object" ? t.requestId : undefined;
    if (typeof requestId !== "string" || !CHAT_ID.test(requestId)) throw bad("memory.extract takes chat turns only (u: request ids).");
    const endedAt = typeof t.endedAt === "string" ? t.endedAt : "";
    if (!turns.some((x) => x.requestId === requestId)) turns.push({ requestId, endedAt });
  }
  if (typeof args.today !== "string" || !DAY.test(args.today)) throw bad("memory.extract needs today as YYYY-MM-DD.");
  const known = (Array.isArray(args.known) ? (args.known as Record<string, unknown>[]) : [])
    .filter((k) => k && typeof k === "object" && typeof k.predicate === "string" && str(k.value) !== null)
    .slice(0, MAX_KNOWN)
    .map((k) => ({ id: Number(k.id), subject: str(k.subject) ?? "user", predicate: k.predicate as string, qualifier: str(k.qualifier), value: str(k.value)! }));
  const registry = (Array.isArray(args.registry) ? (args.registry as Record<string, unknown>[]) : [])
    .filter((r) => r && typeof r === "object" && typeof r.key === "string")
    .map((r) => ({ key: r.key as string, kind: str(r.kind) ?? "profile", one: r.one === true, qualifier: str(r.qualifier) }));
  return { turns, known, registry, today: args.today, episodes: args.episodes === true };
}

/**
 * What the person said and what the bear answered, for each wanted request id, read from the root conversation's
 * full history (never `conv.context`, which compaction shortens), newest first. Only the `buddy.input` entry (its
 * `data.said`, else its user message) and the final assistant message of that run are read: never tool calls or
 * results, events or any other entry.
 */
export async function readTurns(conv: Conversation, wanted: ReadonlySet<string>): Promise<Map<string, { said: string; answer: string }>> {
  const found = new Map<string, { said: string; answer: string }>();
  let answer: string | undefined;
  let cursor: Parameters<Conversation["entries"]>[2];
  for (let page = 0; page < MAX_PAGES && found.size < wanted.size; page++) {
    const res = await conv.entries({ order: "descending" }, PAGE, cursor, ctx);
    for (const e of res.items) {
      if (e.kind === INPUT_KIND) {
        const data = (e.data ?? {}) as { requestId?: unknown; said?: unknown };
        const id = typeof data.requestId === "string" ? data.requestId : "";
        if (wanted.has(id) && !found.has(id)) {
          const user = (e.model ?? []).find((m) => m.role === "user") as TextMessage | undefined;
          const said = typeof data.said === "string" ? data.said : messageText(user);
          found.set(id, { said: clip(said, SAID_MAX), answer: clip(answer ?? "", ANSWER_MAX) });
        }
        answer = undefined;
        continue;
      }
      // The newest assistant message after an input is the run's final one.
      if (answer === undefined) {
        const last = [...(e.model ?? [])].reverse().find((m) => m.role === "assistant");
        if (last) answer = messageText(last as unknown as TextMessage);
      }
    }
    if (!res.next || !res.items.length) break;
    cursor = res.next;
  }
  return found;
}

/** The first JSON object in `text` that has an `items` array, or undefined. */
export function parseItems(text: string): unknown[] | undefined {
  for (const candidate of [text, repair(text)]) {
    const items = firstItems(candidate);
    if (items) return items;
  }
  return undefined;
}

function firstItems(text: string): unknown[] | undefined {
  for (let at = text.indexOf("{"); at !== -1; at = text.indexOf("{", at + 1)) {
    const end = balanced(text, at);
    if (end === -1) continue;
    try {
      const v = JSON.parse(text.slice(at, end + 1));
      if (v && typeof v === "object" && Array.isArray(v.items)) return v.items;
    } catch {
      // not JSON: try the next brace
    }
  }
  return undefined;
}

/** The index of the brace closing the object that opens at `from`, or -1. */
function balanced(text: string, from: number): number {
  let depth = 0;
  let inString = false;
  for (let i = from; i < text.length; i++) {
    const c = text[i];
    if (inString) {
      if (c === "\\") i++;
      else if (c === '"') inString = false;
    } else if (c === '"') inString = true;
    else if (c === "{") depth++;
    else if (c === "}" && --depth === 0) return i;
  }
  return -1;
}

/** The one repair pass: smart quotes, trailing commas, and output cut off mid-list (kept up to its last whole item). */
function repair(text: string): string {
  let t = text.replace(/[“”]/g, '"').replace(/,\s*([}\]])/g, "$1");
  const start = t.indexOf('{"items"') >= 0 ? t.indexOf('{"items"') : t.indexOf("{");
  if (start === -1) return t;
  t = t.slice(start);
  if (balanced(t, 0) !== -1) return t;
  // Cut off: keep the whole items before the last complete one.
  const cut = t.lastIndexOf("}");
  return cut === -1 ? t : `${t.slice(0, cut + 1).replace(/,\s*$/, "")}]}`;
}

/** Map the model's items to the ack shape; an item with no valid exchange or text is dropped. */
export function toItems(raw: unknown[], turns: TurnRef[]): MemoryItem[] {
  const out: MemoryItem[] = [];
  for (const r of raw) {
    if (out.length >= MAX_ITEMS) break;
    if (!r || typeof r !== "object") continue;
    const v = r as Record<string, unknown>;
    const n = Number(v.exchange);
    const turn = Number.isInteger(n) ? turns[n - 1] : undefined;
    const text = str(v.text);
    const kind = str(v.kind);
    if (!turn || !text || !kind) continue;
    out.push({
      turn: turn.requestId,
      kind,
      predicate: str(v.predicate),
      qualifier: str(v.qualifier),
      value: str(v.value),
      text,
      importance: typeof v.importance === "number" ? v.importance : Number(v.importance) || 0,
      validFrom: str(v.valid_from ?? v.validFrom),
      quote: str(v.quote),
    });
  }
  return out;
}

export type TurnRef = { requestId: string };

/** One model call on the current model; text out. Throws CmdError: no_provider, timeout, model_error. */
async function callModel(system: string, user: string): Promise<string> {
  const current = currentModelCall();
  const model = current?.models.getModel(current.ref.provider, current.ref.model);
  if (!current || !model) throw new CmdError("no_provider", "Choose a model for desk-buddy first.");
  const abort = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      abort.abort();
      reject(new CmdError("timeout", "Memory extraction took too long."));
    }, EXTRACT_TIMEOUT_MS);
  });
  try {
    const call = current.models
      .completeSimple(model, { systemPrompt: system, messages: [{ role: "user", content: user, timestamp: Date.now() }] }, { signal: abort.signal, maxTokens: Math.min(model.maxTokens || 4096, 8000) })
      .then((msg) => {
        if (msg.stopReason === "error" || msg.stopReason === "aborted") throw new CmdError("model_error", "The model call failed.");
        return msg.content
          .filter((p) => p.type === "text")
          .map((p) => (p as { text: string }).text)
          .join("");
      });
    call.catch(() => {});
    return await Promise.race([call, timeout]);
  } catch (e) {
    if (e instanceof CmdError) throw e;
    // A missing credential throws before any request.
    throw new CmdError("no_provider", "The chosen model has no sign-in or key.");
  } finally {
    clearTimeout(timer);
  }
}

export async function extract(conv: Conversation, args: Record<string, unknown>): Promise<MemoryExtractAck> {
  const a = parseArgs(args);
  const read = await readTurns(conv, new Set(a.turns.map((t) => t.requestId)));
  const present = a.turns.filter((t) => read.has(t.requestId));
  const missing = a.turns.filter((t) => !read.has(t.requestId)).map((t) => t.requestId);
  const turns = present.map((t) => ({ requestId: t.requestId, said: read.get(t.requestId)!.said }));
  if (!present.length) return { items: [], turns, missing };
  const exchanges: Exchange[] = present.map((t, i) => ({ n: i + 1, endedAt: t.endedAt, said: read.get(t.requestId)!.said, answer: read.get(t.requestId)!.answer }));
  const text = await callModel(extractSystemPrompt(a.registry, a.episodes), extractUserPrompt(a.today, a.known, exchanges));
  const raw = parseItems(text);
  if (!raw) throw new CmdError("bad_output", "The model's answer had no items list.");
  return { items: toItems(raw, present), turns, missing };
}
