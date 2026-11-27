// Recall on a chat message (design I1, p2): Rust picks up to 3 notes from earlier chats and sends them with the
// `prompt` command; the sidecar appends them to the input it submits, after the person's words. The entry's
// `data.said` keeps the person's words alone, so the bubble, the transcript and the extractor never see the block.
// Nothing here logs the lines.

import { CmdError } from "../../protocol.ts";

export const RECALL_MAX_LINES = 3;
/** All lines together, in characters. */
export const RECALL_MAX_CHARS = 600;
export const RECALL_HEAD = "[Notes from earlier chats that may matter. Use them only if they help;\ndon't mention them otherwise.]";
const LINE = /^\d{4}-\d{2}-\d{2}: \S/;

/** The `recall` field: absent → []; else 1..3 lines "YYYY-MM-DD: text", ≤ 600 chars in all, or "bad_request". */
export function parseRecall(raw: unknown): string[] {
  if (raw == null) return [];
  if (!Array.isArray(raw)) throw new CmdError("bad_request", "recall must be a list of lines.");
  if (raw.length > RECALL_MAX_LINES) throw new CmdError("bad_request", `recall takes at most ${RECALL_MAX_LINES} lines.`);
  let chars = 0;
  for (const l of raw) {
    if (typeof l !== "string" || !LINE.test(l) || l.includes("\n")) throw new CmdError("bad_request", 'Each recall line is "YYYY-MM-DD: text" on one line.');
    chars += [...l].length;
  }
  if (chars > RECALL_MAX_CHARS) throw new CmdError("bad_request", `recall is over ${RECALL_MAX_CHARS} characters.`);
  return raw as string[];
}

/** The content the model gets: the person's words, then the block when there are lines. */
export function withRecall(text: string, recall: readonly string[]): string {
  if (!recall.length) return text;
  return `${text}\n\n${RECALL_HEAD}\n${recall.map((l) => `- ${l}`).join("\n")}`;
}
