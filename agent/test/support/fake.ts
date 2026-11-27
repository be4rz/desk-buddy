// A scripted fake model provider (design §2.8): OpenAI Chat Completions
// (`POST …/chat/completions`) and Anthropic Messages (`POST …/messages`),
// streaming. Every request is recorded byte-exact. No paid call ever leaves it.
//
//   startFake({ script })                     in tests
//   bun agent/test/support/fake.ts --port <p> --script <file.json> [--log <file.jsonl>]   for QA in the real app
//     (--log appends {path, body} of every request as one JSON line, for tests in another process)
//
// A script entry matches the newest user or tool message (`match`: a substring
// or RegExp; in a JSON file "/re/flags"); the first matching entry with `times`
// left replies. No match: "OK.".

import { appendFileSync } from "node:fs";

export type Reply = {
  text?: string;
  reasoning?: string;
  toolCalls?: { name: string; args: Record<string, unknown> }[];
  usage?: { input: number; output: number; cacheRead?: number; cacheWrite?: number };
  error?: { status: number; body: unknown; headers?: Record<string, string> };
  /** Before every streamed chunk after the first. */
  delayMs?: number;
};
export type ScriptEntry = { match?: string | RegExp; reply: Reply; times?: number };
export type RecordedRequest = { at: number; method: string; path: string; headers: Record<string, string>; body: string };
export type Fake = { port: number; url: string; requests(): RecordedRequest[]; setScript(script: ScriptEntry[]): void; stop(): void };

type Msg = { role: string; content: unknown };

const text = (c: unknown): string => {
  if (typeof c === "string") return c;
  if (Array.isArray(c))
    return c
      .map((p: any) => (typeof p === "string" ? p : p?.type === "text" ? p.text : p?.type === "tool_result" ? text(p.content) : ""))
      .join("");
  return c == null ? "" : JSON.stringify(c);
};

/** The newest user or tool message's text, in either API's shape. */
function newest(messages: Msg[]): string {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === "user" || m.role === "tool") return text(m.content);
  }
  return "";
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function stream(chunks: string[], delayMs = 0) {
  const enc = new TextEncoder();
  let i = 0;
  return new Response(
    new ReadableStream({
      async pull(controller) {
        if (i >= chunks.length) return controller.close();
        if (i > 0 && delayMs) await sleep(delayMs);
        controller.enqueue(enc.encode(chunks[i++]));
      },
    }),
    { headers: { "content-type": "text/event-stream", "cache-control": "no-cache" } },
  );
}

let calls = 0;

function openai(reply: Reply, model: string): string[] {
  const base = { id: "chatcmpl-fake", object: "chat.completion.chunk", created: 0, model };
  const out: unknown[] = [];
  const delta = (d: Record<string, unknown>, finish: string | null = null) => out.push({ ...base, choices: [{ index: 0, delta: d, finish_reason: finish }] });
  delta({ role: "assistant", content: "" });
  if (reply.reasoning) for (const part of halves(reply.reasoning)) delta({ reasoning_content: part });
  if (reply.text) for (const part of halves(reply.text)) delta({ content: part });
  (reply.toolCalls ?? []).forEach((c, index) => {
    const args = JSON.stringify(c.args);
    const [a, b] = halves(args);
    delta({ tool_calls: [{ index, id: `call_${++calls}`, type: "function", function: { name: c.name, arguments: a } }] });
    if (b) delta({ tool_calls: [{ index, function: { arguments: b } }] });
  });
  delta({}, reply.toolCalls?.length ? "tool_calls" : "stop");
  const u = reply.usage ?? { input: 100, output: 10 };
  out.push({ ...base, choices: [], usage: { prompt_tokens: u.input, completion_tokens: u.output, prompt_tokens_details: { cached_tokens: u.cacheRead ?? 0 } } });
  return [...out.map((c) => `data: ${JSON.stringify(c)}\n\n`), "data: [DONE]\n\n"];
}

function anthropic(reply: Reply, model: string): string[] {
  const u = reply.usage ?? { input: 100, output: 10 };
  const ev = (type: string, data: Record<string, unknown>) => `event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`;
  const out = [
    ev("message_start", {
      message: { id: "msg_fake", type: "message", role: "assistant", model, content: [], stop_reason: null, usage: { input_tokens: u.input, output_tokens: 0, cache_read_input_tokens: u.cacheRead ?? 0, cache_creation_input_tokens: u.cacheWrite ?? 0 } },
    }),
  ];
  let index = 0;
  if (reply.reasoning) {
    out.push(ev("content_block_start", { index, content_block: { type: "thinking", thinking: "" } }));
    for (const part of halves(reply.reasoning)) out.push(ev("content_block_delta", { index, delta: { type: "thinking_delta", thinking: part } }));
    out.push(ev("content_block_delta", { index, delta: { type: "signature_delta", signature: "fake-signature" } }));
    out.push(ev("content_block_stop", { index: index++ }));
  }
  if (reply.text) {
    out.push(ev("content_block_start", { index, content_block: { type: "text", text: "" } }));
    for (const part of halves(reply.text)) out.push(ev("content_block_delta", { index, delta: { type: "text_delta", text: part } }));
    out.push(ev("content_block_stop", { index: index++ }));
  }
  for (const c of reply.toolCalls ?? []) {
    out.push(ev("content_block_start", { index, content_block: { type: "tool_use", id: `toolu_${++calls}`, name: c.name, input: {} } }));
    for (const part of halves(JSON.stringify(c.args))) out.push(ev("content_block_delta", { index, delta: { type: "input_json_delta", partial_json: part } }));
    out.push(ev("content_block_stop", { index: index++ }));
  }
  out.push(ev("message_delta", { delta: { stop_reason: reply.toolCalls?.length ? "tool_use" : "end_turn" }, usage: { output_tokens: u.output } }));
  out.push(ev("message_stop", {}));
  return out;
}

function halves(s: string): string[] {
  if (s.length < 2) return [s];
  const mid = Math.ceil(s.length / 2);
  return [s.slice(0, mid), s.slice(mid)];
}

function matches(m: ScriptEntry["match"], s: string) {
  if (m === undefined) return true;
  return typeof m === "string" ? s.includes(m) : m.test(s);
}

export function startFake({ script = [], port = 0, log }: { script?: ScriptEntry[]; port?: number; log?: string } = {}): Fake {
  let entries = script.map((e) => ({ ...e }));
  const recorded: RecordedRequest[] = [];
  const server = Bun.serve({
    port,
    hostname: "127.0.0.1",
    idleTimeout: 0,
    async fetch(req) {
      const body = await req.text();
      const path = new URL(req.url).pathname;
      recorded.push({ at: Date.now(), method: req.method, path, headers: Object.fromEntries(req.headers), body });
      if (log) appendFileSync(log, `${JSON.stringify({ path, body })}\n`);
      if (req.method !== "POST") return new Response("not found", { status: 404 });
      let json: any;
      try {
        json = JSON.parse(body);
      } catch {
        return new Response("bad json", { status: 400 });
      }
      const messages: Msg[] = json.messages ?? [];
      const last = newest(messages);
      const entry = entries.find((e) => (e.times === undefined || e.times > 0) && matches(e.match, last));
      if (entry?.times !== undefined) entry.times--;
      const reply: Reply = entry?.reply ?? { text: "OK." };
      if (reply.delayMs && !reply.text && !reply.toolCalls && !reply.reasoning) await sleep(reply.delayMs);
      if (reply.error)
        return new Response(typeof reply.error.body === "string" ? reply.error.body : JSON.stringify(reply.error.body), {
          status: reply.error.status,
          headers: { "content-type": "application/json", ...reply.error.headers },
        });
      const model = String(json.model ?? "fake-1");
      if (path.endsWith("/messages")) return stream(anthropic(reply, model), reply.delayMs);
      return stream(openai(reply, model), reply.delayMs);
    },
  });
  return {
    port: server.port!,
    url: `http://127.0.0.1:${server.port}`,
    requests: () => recorded.slice(),
    setScript: (s) => {
      entries = s.map((e) => ({ ...e }));
    },
    stop: () => server.stop(true),
  };
}

/** A script from JSON: a `match` of the form "/re/flags" becomes a RegExp. */
export function parseScript(json: unknown): ScriptEntry[] {
  return (json as ScriptEntry[]).map((e) => {
    const m = typeof e.match === "string" ? /^\/(.*)\/([a-z]*)$/s.exec(e.match) : null;
    return m ? { ...e, match: new RegExp(m[1], m[2]) } : e;
  });
}

if (import.meta.main) {
  const arg = (name: string) => {
    const i = process.argv.indexOf(name);
    return i > 0 ? process.argv[i + 1] : undefined;
  };
  const file = arg("--script");
  const script = file ? parseScript(JSON.parse(await Bun.file(file).text())) : [];
  const fake = startFake({ script, port: Number(arg("--port") ?? 0), log: arg("--log") });
  console.error(`fake provider on ${fake.url} (${script.length} script entries); OpenAI base ${fake.url}/v1`);
}
