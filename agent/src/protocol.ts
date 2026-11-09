// The JSONL records between the Rust host and the sidecar. One JSON object per
// line, split only on "\n": CR, U+2028 and U+0085 inside strings never end a
// record. Rust mirrors these in crates/bridge/src/protocol.rs.
//
// Ported from 2market (agent/src/protocol.ts). pi-durable 1.5 removed its
// event stream, so `ev` records are derived from the conversation view:
// `status` (what the run is doing), `text` (the streamed answer's new text)
// and `entry` (a committed transcript entry).

export const V = 1;

export type ModelRef = { provider: string; model: string };

export type ProviderModelDef = {
  id: string;
  name: string;
  contextWindow: number;
  maxTokens: number;
  reasoning: boolean;
  input: ("text" | "image")[];
  /** pi-ai's per-model `compat` flags, merged over the endpoint's own; tests set them for fake models. */
  compat?: Record<string, unknown>;
};

/** An endpoint pi-ai lacks, or a debug base override; never a key. */
export type ProviderDef = {
  id: string;
  name: string;
  api: "openai-completions" | "openai-responses" | "anthropic-messages";
  baseUrl: string;
  models: ProviderModelDef[];
};

/** How the agent's commands reach the host API (plan §2 "Host API auth"). */
export type HostApi = {
  /** `http://127.0.0.1:<port>`. */
  url: string;
  /** This sidecar's agent token; revoked when it exits. Only ever placed in a command's exact env. */
  token: string;
};

export type Init = {
  t: "init";
  v: number;
  agentId: string;
  /** The agent's working folder: its cwd and the root file rules resolve against. */
  workDir: string;
  storagePath: string;
  skillsDir: string;
  model: ModelRef | null;
  providers: ProviderDef[];
  /** The whole environment of every shell command; nothing is inherited from the sidecar. */
  shellEnv: Record<string, string>;
  /** pi-durable `shellPath`: busybox-w32's `sh` on Windows (D10), `/bin/bash` elsewhere. */
  shellPath: string;
  platform: "macos" | "windows" | "linux";
  host: HostApi | null;
  /** Open: later tickets add tunables, each with a default here. */
  settings: Settings;
};

export type Settings = {
  stepLimit?: number;
  shutdownMs?: number;
  /** How long `policy.decide` may take before the call is blocked (ms). */
  decideMs?: number;
  /** pi-durable `settings.retry.maxRetries`; 1 so a generation cut off by a crash runs once more. */
  maxRetries?: number;
  /** pi-durable `settings.retry.baseDelayMs`; tests make it short. */
  retryDelayMs?: number;
  /** How the previous sidecar of this agent ended, for the `interrupted` event. */
  lastExit?: "crash" | "quit" | "clean";
  [k: string]: unknown;
};

/** What the run is doing, derived from `pi.live` (the mascot's source of truth). */
export type Status = {
  busy: boolean;
  /** `idle`, `thinking` (waiting for the model), `speaking` (answer text streaming), `tool` (a call running). */
  phase: "idle" | "thinking" | "speaking" | "tool";
  /** The running calls the gate let through, in call order. */
  tools: { callId: string; name: string; command?: string; path?: string }[];
  /** Calls waiting for the person's answer to their approval card. */
  asking: string[];
  /** Why the last run ended without an answer; until the next run starts. */
  failure?: { reason: string; detail?: string };
  /** While busy: the request id of the input the run answers. */
  turn?: string;
};

export type Cmd = { t: "cmd"; id: string; op: string; [k: string]: unknown };
export type Res = { t: "res"; id: string; ok: true; result: unknown } | { t: "res"; id: string; ok: false; error: { code: string; message: string } };

export type Out =
  | { t: "ack"; id: string; ok: true; result?: unknown }
  | { t: "ack"; id: string; ok: false; error: { code: string; message: string } }
  | { t: "ev"; seq: number; kind: "status"; status: Status }
  /** New answer text since the last `text` record; `reset` when a new attempt starts over. */
  | { t: "ev"; seq: number; kind: "text"; delta: string; reset?: true }
  | { t: "ev"; seq: number; kind: "entry"; entry: unknown }
  | { t: "req"; id: string; op: string; [k: string]: unknown }
  | { t: "cancel"; id: string }
  | { t: "ready"; v: number; pid: number; conversationId: string }
  | { t: "fatal"; code: string; message: string };

/** The only accepted request id forms: a chat message, a Continue, a Retry. */
const REQUEST_ID = /^(u:[0-9a-f-]{36}|continue:[A-Za-z0-9_.:-]+|retry:[A-Za-z0-9_.:-]+)$/;

export function requestSource(requestId: string): string | undefined {
  if (!REQUEST_ID.test(requestId)) return undefined;
  const prefix = requestId.slice(0, requestId.indexOf(":"));
  return prefix === "u" ? "chat" : prefix;
}

export class CmdError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** Settles when stdout has taken the newest record. */
let lastWrite: Promise<void> = Promise.resolve();
let writer: (line: string) => void = (line) => {
  lastWrite = new Promise((resolve) => process.stdout.write(line, () => resolve()));
};

/** Tests can capture records instead of writing stdout. */
export function setWriter(w: (line: string) => void) {
  writer = w;
}

export function send(record: Out) {
  writer(JSON.stringify(record) + "\n");
}

/** Resolves once stdout has taken every record written so far; call before exiting. */
export function flushed(): Promise<void> {
  return lastWrite;
}

/** Exit after stdout drains, so a pipe never loses the last records. */
export async function exit(code: number): Promise<never> {
  await Promise.race([flushed(), new Promise((r) => setTimeout(r, 2000))]);
  process.exit(code);
}

/** Read stdin as LF-separated JSON records. Lines that aren't JSON objects are skipped. */
export async function* records(input: AsyncIterable<Uint8Array | string> = process.stdin): AsyncGenerator<Record<string, unknown>> {
  const decoder = new TextDecoder();
  let pending: Uint8Array[] = [];
  const flush = (bytes: Uint8Array[]) => {
    const total = bytes.reduce((n, b) => n + b.length, 0);
    const all = new Uint8Array(total);
    let at = 0;
    for (const b of bytes) {
      all.set(b, at);
      at += b.length;
    }
    return decoder.decode(all);
  };
  for await (const chunk of input) {
    let bytes = typeof chunk === "string" ? new TextEncoder().encode(chunk) : chunk;
    let nl: number;
    while ((nl = bytes.indexOf(0x0a)) !== -1) {
      pending.push(bytes.subarray(0, nl));
      const line = flush(pending);
      pending = [];
      bytes = bytes.subarray(nl + 1);
      const parsed = parse(line);
      if (parsed) yield parsed;
    }
    if (bytes.length) pending.push(bytes.slice());
  }
  if (pending.length) {
    const parsed = parse(flush(pending));
    if (parsed) yield parsed;
  }
}

function parse(line: string): Record<string, unknown> | undefined {
  if (!line.trim()) return undefined;
  try {
    const v = JSON.parse(line);
    return v && typeof v === "object" && !Array.isArray(v) ? v : undefined;
  } catch {
    return undefined;
  }
}
