// The text of a pi-ai message, for the status line and the helpers' answers.

type Part = { type: string; text?: string };
export type TextMessage = { role: string; content: string | Part[] };

/** The text of a message's text parts. */
export function messageText(m: TextMessage | undefined): string {
  if (!m) return "";
  if (typeof m.content === "string") return m.content;
  return m.content
    .filter((p) => p.type === "text")
    .map((p) => p.text ?? "")
    .join("");
}
