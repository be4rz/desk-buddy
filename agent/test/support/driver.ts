// Spawns the compiled `buddy-agent` in a temp folder and plays Rust: sends
// `init`, answers `cred.*` from memory and `policy.*` with the test's handlers.
// Ported from 2market's test driver.

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Init } from "../../src/protocol.ts";
import { records } from "../../src/protocol.ts";

const agentDir = join(import.meta.dir, "..", "..");

/** The binary under test: `BUDDY_AGENT_BIN`, else a fresh build of `agent/src`. */
export const BIN = (() => {
  const set = process.env.BUDDY_AGENT_BIN;
  if (set) {
    if (!existsSync(set)) throw new Error(`BUDDY_AGENT_BIN=${set} does not exist`);
    return set;
  }
  const out = join(agentDir, "dist", "buddy-agent");
  const built = spawnSync("bun", [join(agentDir, "scripts", "build.ts")], { stdio: ["ignore", "ignore", "inherit"] });
  if (built.status !== 0) throw new Error("could not build the buddy-agent sidecar");
  return out;
})();

export type Rec = { t: string; [k: string]: any };
export type Handlers = {
  decide?: (args: Rec) => unknown | Promise<unknown>;
  wait?: (args: Rec) => unknown | Promise<unknown>;
  /** `policy.taint`; default: newly the first time. */
  taint?: (args: Rec) => unknown | Promise<unknown>;
};

export const ALLOW_ALL: Handlers = { decide: () => ({ decision: "allow" }) };

export const FAKE_MODELS = [{ id: "fake-1", name: "Fake 1", contextWindow: 128000, maxTokens: 4096, reasoning: false, input: ["text"] as ("text" | "image")[] }];

export type DriverOptions = {
  /** The fake's base URL (no `/v1`). */
  fakeUrl: string;
  credentials?: Record<string, unknown>;
  handlers?: Handlers;
  settings?: Init["settings"];
  shellEnv?: Record<string, string>;
  host?: Init["host"];
  model?: Init["model"];
  /** The fake provider's models; default FAKE_MODELS. */
  models?: typeof FAKE_MODELS;
};

export class Driver {
  readonly dir: string;
  readonly home: string;
  readonly storagePath: string;
  /** Every record of the current sidecar, in order. */
  records: Rec[] = [];
  /** Records of earlier sidecars, by start. */
  past: Rec[][] = [];
  /** Every `req` the sidecar sent, across restarts. */
  reqs: Rec[] = [];
  child!: ChildProcess;
  exited!: Promise<number | null>;
  credentials: Map<string, unknown>;
  stderr = "";
  /** Whether the default `policy.taint` handler marked the conversation yet. */
  tainted = false;
  private cmds = 0;
  private acks = new Map<string, (r: Rec) => void>();

  constructor(readonly opts: DriverOptions) {
    this.dir = mkdtempSync(join(tmpdir(), "buddy-work-"));
    this.home = `${this.dir}-home`;
    mkdirSync(this.home, { recursive: true });
    this.storagePath = join(this.home, "agents", "a1", "agent.sqlite");
    this.credentials = new Map(Object.entries(opts.credentials ?? { fake: { type: "api_key", key: "sk-fake-credential-7c1f" } }));
  }

  init(settings: Init["settings"] = {}): Init {
    const o = this.opts;
    return {
      t: "init",
      v: 1,
      agentId: "a1",
      workDir: this.dir,
      storagePath: this.storagePath,
      skillsDir: join(agentDir, "skills"),
      model: o.model === undefined ? { provider: "fake", model: "fake-1" } : o.model,
      providers: [{ id: "fake", name: "Fake", api: "openai-completions", baseUrl: `${o.fakeUrl}/v1`, models: o.models ?? FAKE_MODELS }],
      shellEnv: o.shellEnv ?? { PATH: process.env.PATH ?? "/usr/bin:/bin", HOME: this.home, LANG: "en_US.UTF-8", TERM: "dumb" },
      shellPath: "/bin/bash",
      platform: process.platform === "darwin" ? "macos" : process.platform === "win32" ? "windows" : "linux",
      host: o.host ?? null,
      settings: { stepLimit: 50, shutdownMs: 3000, retryDelayMs: 50, ...o.settings, ...settings },
    };
  }

  async start(settings: Init["settings"] = {}) {
    if (this.records.length) this.past.push(this.records);
    this.records = [];
    this.stderr = "";
    // Rust's env_clear() plus the base set: no key in the sidecar's environment.
    const env: Record<string, string> = { HOME: this.home, PATH: process.env.PATH ?? "/usr/bin:/bin", TMPDIR: tmpdir(), LANG: "en_US.UTF-8" };
    // Its own process group, as Rust spawns it, so kill9() takes its commands too.
    this.child = spawn(BIN, [], { cwd: this.dir, env, stdio: ["pipe", "pipe", "pipe"], detached: true });
    this.exited = new Promise((r) => this.child.on("exit", (code) => r(code)));
    this.child.stderr!.on("data", (b) => {
      this.stderr += String(b);
    });
    const child = this.child;
    void (async () => {
      for await (const rec of records(child.stdout! as AsyncIterable<Uint8Array>)) this.onRecord(rec as Rec);
    })();
    this.write(this.init(settings));
    return this.waitFor((r) => r.t === "ready" || r.t === "fatal", 20000);
  }

  write(rec: object) {
    if (this.child.stdin!.writable) this.child.stdin!.write(`${JSON.stringify(rec)}\n`);
  }

  private onRecord(rec: Rec) {
    this.records.push(rec);
    if (rec.t === "ack") {
      this.acks.get(rec.id)?.(rec);
      this.acks.delete(rec.id);
    } else if (rec.t === "req") {
      this.reqs.push(rec);
      void this.answer(rec);
    }
  }

  private async answer(req: Rec) {
    const h = this.opts.handlers ?? ALLOW_ALL;
    const ok = (result: unknown) => this.write({ t: "res", id: req.id, ok: true, result });
    const err = (code: string, message: string) => this.write({ t: "res", id: req.id, ok: false, error: { code, message } });
    try {
      switch (req.op) {
        case "cred.read":
          return ok(this.credentials.get(req.providerId) ?? null);
        case "cred.list":
          return ok([...this.credentials].map(([providerId, c]) => ({ providerId, type: (c as Rec).type })));
        case "cred.modify_begin":
          return ok({ lease: `l-${req.providerId}`, credential: this.credentials.get(req.providerId) ?? null });
        case "cred.modify_end": {
          const providerId = String(req.lease).slice(2);
          if (req.write) this.credentials.set(providerId, req.credential);
          return ok({ credential: this.credentials.get(providerId) ?? null });
        }
        case "cred.delete":
          this.credentials.delete(req.providerId);
          return ok({});
        case "policy.decide":
          return h.decide ? ok(await h.decide(req)) : ok({ decision: "deny", reason: "No policy in this test." });
        case "policy.wait":
          return h.wait ? ok(await h.wait(req)) : err("unsupported", "No approvals in this test.");
        case "policy.taint": {
          if (h.taint) return ok(await h.taint(req));
          const newly = !this.tainted;
          this.tainted = true;
          return ok({ newly });
        }
        default:
          return err("unknown_op", req.op);
      }
    } catch (e) {
      return err("error", e instanceof Error ? e.message : String(e));
    }
  }

  /** Send a command; resolves with its ack. */
  send(op: string, args: Record<string, unknown> = {}): Promise<Rec> {
    const id = `c-${++this.cmds}`;
    const acked = new Promise<Rec>((r) => this.acks.set(id, r));
    this.write({ t: "cmd", id, op, ...args });
    return acked;
  }

  prompt(text: string, requestId = `u:${crypto.randomUUID()}`) {
    return this.send("prompt", { requestId, text, source: "chat" });
  }

  async waitFor(pred: (r: Rec) => boolean, ms = 15000): Promise<Rec> {
    const end = Date.now() + ms;
    for (;;) {
      const hit = this.records.find(pred);
      if (hit) return hit;
      if (Date.now() > end) throw new Error(`timed out; last records:\n${this.records.slice(-8).map((r) => JSON.stringify(r).slice(0, 300)).join("\n")}\nstderr:\n${this.stderr.slice(-2000)}`);
      await new Promise((r) => setTimeout(r, 20));
    }
  }

  /** Committed entries sent live by this sidecar. */
  entries(): Rec[] {
    return this.records.filter((r) => r.t === "ev" && r.kind === "entry").map((r) => r.entry);
  }

  /** The buddy events among them. */
  events(): Rec[] {
    return this.entries()
      .filter((e) => e.kind === "buddy.event")
      .map((e) => e.data);
  }

  statuses(): Rec[] {
    return this.records.filter((r) => r.t === "ev" && r.kind === "status").map((r) => r.status);
  }

  /** Waits for a run to start and end after `after` (a record count). */
  async waitRunEnd(after = 0, ms = 20000) {
    await this.waitFor((r) => this.records.indexOf(r) >= after && r.t === "ev" && r.kind === "status" && r.status.busy, ms);
    const busyAt = this.records.findIndex((r, i) => i >= after && r.t === "ev" && r.kind === "status" && r.status.busy);
    return this.waitFor((r) => this.records.indexOf(r) > busyAt && r.t === "ev" && r.kind === "status" && !r.status.busy, ms);
  }

  async snapshot(): Promise<Rec[]> {
    const ack = await this.send("snapshot");
    return ack.result.entries;
  }

  /** SIGKILL the sidecar's process group: the sidecar and the commands it started. */
  kill9() {
    try {
      process.kill(-this.child.pid!, "SIGKILL");
    } catch {
      this.child.kill("SIGKILL");
    }
    return this.exited;
  }

  async shutdown() {
    await this.send("shutdown");
    return this.exited;
  }
}

/** Tool calls in a transcript with no result entry. */
export function unanswered(entries: Rec[]) {
  const calls = entries.flatMap((e) => (e.model ?? []).flatMap((m: Rec) => (m.role === "assistant" ? m.content.filter((c: Rec) => c.type === "toolCall").map((c: Rec) => c.id) : [])));
  const results = new Set(entries.flatMap((e) => (e.model ?? []).filter((m: Rec) => m.role === "toolResult").map((m: Rec) => m.toolCallId)));
  return calls.filter((id: string) => !results.has(id));
}

/** The text of every tool result, in order. */
export function toolResults(entries: Rec[]): { text: string; isError: boolean }[] {
  return entries
    .flatMap((e) => e.model ?? [])
    .filter((m: Rec) => m.role === "toolResult")
    .map((m: Rec) => ({ text: m.content.map((c: Rec) => c.text ?? "").join(""), isError: !!m.isError }));
}

/** The text of the last assistant answer. */
export function lastAnswer(entries: Rec[]): string {
  const m = entries
    .flatMap((e) => e.model ?? [])
    .filter((m: Rec) => m.role === "assistant")
    .at(-1);
  return m ? m.content.filter((c: Rec) => c.type === "text").map((c: Rec) => c.text).join("") : "";
}
