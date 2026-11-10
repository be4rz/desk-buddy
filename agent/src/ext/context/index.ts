// buddy.context (design §4.7): keeps the model's context small.
// - Capping: a tool result over `maxLines` lines or `maxBytes` bytes keeps its
//   head and a footer naming `.buddy/outputs/<callId>.txt`, where the full text
//   is written. Nested calls are left alone: their caller sees them, not the model.
//   A helper conversation (`task`) has no `read`, so its results are cut with a
//   footer that says so and no file is written.
// - Clearing: once a request's estimate passes 60% of the model's window, the
//   oldest tool results become `[cleared]`, a whole batch at a time, so the
//   request prefix changes rarely (provider caches stay warm). The newest batch
//   is never cleared.
// Compaction itself is pi-durable's; `compactionSettings()` sizes it per model.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { defineExtension, GenerationTask, hook, ToolTask, type ToolExecutionResult } from "@earendil-works/pi-durable";
import { contextWindow } from "../../current.ts";
import { rootId } from "../core/index.ts";
import { CONTEXT_DEFAULTS, type ContextSettings, type Init } from "../../protocol.ts";

export const CLEARED = "[cleared]";
/** Share of the window past which old tool results are cleared. */
export const CLEAR_AT = 0.6;

export function contextSettings(init: Init): ContextSettings {
  const s = init.settings.context ?? {};
  const pick = (k: keyof ContextSettings) => (typeof s[k] === "number" && (s[k] as number) > 0 ? (s[k] as number) : CONTEXT_DEFAULTS[k]);
  return { maxLines: pick("maxLines"), maxBytes: pick("maxBytes"), clearBatch: pick("clearBatch"), compactPct: Math.min(95, pick("compactPct")) };
}

/** pi-durable's compaction policy for a window: compact at `compactPct`% of it. */
export function compactionFor(cw: number, compactPct: number) {
  return {
    reserveTokens: Math.ceil(((100 - compactPct) / 100) * cw),
    keepRecentTokens: Math.min(20000, Math.ceil(0.25 * cw)),
    backgroundTokens: Math.min(32768, Math.ceil(0.1 * cw)),
  };
}

/** `settings.compaction`, read at every use, so a model change resizes it. */
export function compactionSettings(init: Init) {
  const { compactPct } = contextSettings(init);
  return compactionFor(contextWindow(), compactPct);
}

/** A file name from a call id: only `[A-Za-z0-9_.-]`, never a path. */
export function outputName(callId: string): string {
  const safe = callId.replace(/[^A-Za-z0-9_.-]/g, "_").replace(/^\.+/, "_").slice(0, 120);
  return `${safe || "call"}.txt`;
}

/** Cut `text` to its first `maxLines` lines and `maxBytes` UTF-8 bytes (never mid-character). */
export function head(text: string, maxLines: number, maxBytes: number): string {
  let out = text.split("\n").slice(0, maxLines).join("\n");
  const bytes = Buffer.from(out, "utf8");
  if (bytes.length > maxBytes) {
    let end = maxBytes;
    while (end > 0 && (bytes[end]! & 0xc0) === 0x80) end--;
    out = bytes.subarray(0, end).toString("utf8");
  }
  return out;
}

type Content = NonNullable<ToolExecutionResult["output"]>;

/** The capped result, or undefined when it fits. Writes the full text when it caps and `save` (a helper can't read it back). */
export function capResult(result: ToolExecutionResult, callId: string, workDir: string, s: ContextSettings, save = true): ToolExecutionResult | undefined {
  const output = (result.output ?? []) as Content;
  const text = output.map((p) => (p.type === "text" ? p.text : "")).join("");
  // A final newline ends the last line rather than starting another.
  const lines = text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
  if (lines <= s.maxLines && Buffer.byteLength(text, "utf8") <= s.maxBytes) return undefined;
  const name = outputName(callId);
  const rel = `.buddy/outputs/${name}`;
  let footer: string;
  if (!save) footer = "[output capped: the rest of this result isn't shown]";
  else try {
    const dir = join(workDir, ".buddy", "outputs");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, name), text);
    footer = `[output capped: full text in ${rel}]`;
  } catch (e) {
    footer = `[output capped: the full text could not be saved (${e instanceof Error ? e.message : String(e)})]`;
  }
  const kept = head(text, s.maxLines, Math.max(0, s.maxBytes - footer.length - 2));
  const images = output.filter((p) => p.type !== "text");
  return { ...result, output: [{ type: "text", text: `${kept}\n\n${footer}` }, ...images] as Content };
}

type Msg = { role: string; content?: unknown };

/** Rough tokens of a request: characters / 4. */
export function estimateTokens(messages: readonly Msg[]): number {
  let chars = 0;
  for (const m of messages) {
    if (typeof m.content === "string") chars += m.content.length;
    else if (Array.isArray(m.content))
      for (const p of m.content as { type: string; text?: string; data?: string; arguments?: unknown }[]) {
        if (typeof p.text === "string") chars += p.text.length;
        else if (p.type === "image") chars += 4 * 1600; // about what providers charge for one image
        else if (p.type === "toolCall") chars += JSON.stringify(p.arguments ?? {}).length;
      }
  }
  return Math.ceil(chars / 4);
}

/**
 * The request with its oldest tool results cleared, whole batches at a time, until it fits under `CLEAR_AT` of the
 * window; undefined when nothing changes. The newest `batch` results stay.
 */
export function clearOld<M extends Msg>(messages: readonly M[], cw: number, batch: number): M[] | undefined {
  const limit = CLEAR_AT * cw;
  let est = estimateTokens(messages);
  if (est <= limit) return undefined;
  const results = messages.flatMap((m, i) => (m.role === "toolResult" && !isCleared(m) ? [i] : []));
  const clearable = results.slice(0, Math.max(0, results.length - batch));
  if (clearable.length < batch) return undefined;
  const out = [...messages];
  for (let at = 0; at + batch <= clearable.length && est > limit; at += batch) {
    for (const i of clearable.slice(at, at + batch)) out[i] = { ...out[i]!, content: [{ type: "text", text: CLEARED }] };
    est = estimateTokens(out);
  }
  return out;
}

function isCleared(m: Msg): boolean {
  return Array.isArray(m.content) && m.content.length === 1 && (m.content[0] as { text?: string }).text === CLEARED;
}

export function createContext(init: Init) {
  const s = contextSettings(init);
  return defineExtension({
    name: "buddy.context",
    hooks: [
      hook(GenerationTask, {
        beforeRequest: ({ messages }) => {
          const cleared = clearOld(messages as readonly Msg[], contextWindow(), s.clearBatch);
          return cleared ? { messages: cleared as unknown as typeof messages } : undefined;
        },
      }),
      hook(ToolTask, {
        afterTool: (call, result, api) =>
          call.parent ? undefined : capResult(result, call.id, init.workDir, s, String(api.conversationId) === rootId()),
      }),
    ],
  });
}
