// The sidecar's one mode: one desk-buddy agent. Opens its storage, resumes any
// unfinished work, turns the root conversation's view into `ev` records and
// answers Rust's commands. Ported from 2market's host.ts to pi-durable 1.5,
// which replaced the event stream with the conversation view.

import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { type Conversation, type ConversationWatch, Harness, type LiveState, type ModelRef as PiModelRef } from "@earendil-works/pi-durable";
import { SqliteStorage } from "@earendil-works/pi-durable/storage/sqlite";
import { RustCredentialStore } from "./credentials.ts";
import { executionEnv } from "./env.ts";
import { bindRoot, command, EVENT_KIND, INPUT_KIND, recordEvent } from "./ext/core/index.ts";
import { currentTrigger, gate, markAsked, setTrigger, type Trigger } from "./ext/gate/index.ts";
import { buildModels } from "./models.ts";
import { CmdError, type Cmd, exit, type Init, type ModelRef, requestSource, send, type Status, V } from "./protocol.ts";
import { buildRegistry } from "./registry.ts";
import { failAll, settle } from "./rpc.ts";
import { openBunSqliteDatabase } from "./storage/bun-sqlite.ts";

const ctx = BACKGROUND_CONTEXT;

const piModel = (m: ModelRef | null | undefined): PiModelRef | null => (m ? { provider: m.provider, modelId: m.model } : null);

type Part = { type: string; text?: string; id?: string; name?: string; arguments?: Record<string, unknown> };
type Message = { role: string; content: string | Part[] };
export type Entry = { id: string; kind: string; data?: any; model?: Message[] };

/** The text of a message's text parts. */
export function messageText(m: Message | undefined): string {
  if (!m) return "";
  if (typeof m.content === "string") return m.content;
  return m.content
    .filter((p) => p.type === "text")
    .map((p) => p.text ?? "")
    .join("");
}

/** What the run is doing, from `pi.live`, the gate and the arguments of the round's calls. */
export function statusOf(live: LiveState | undefined, args: Map<string, Record<string, unknown>>, g: { cleared: Set<string>; asking: Set<string> } = gate): Status {
  const failure = live?.runFailure ? { reason: live.runFailure.reason, ...(live.runFailure.detail ? { detail: live.runFailure.detail } : {}) } : undefined;
  if (!live?.run) return { busy: false, phase: "idle", tools: [], asking: [], ...(failure ? { failure } : {}) };
  const slots = live.tools ?? [];
  // Forget cleared calls whose round is over.
  for (const id of g.cleared) if (!slots.some((t) => t.callId === id)) g.cleared.delete(id);
  const asking = slots.filter((t) => g.asking.has(t.callId)).map((t) => t.callId);
  const running = slots.filter((t) => t.status === "running" && g.cleared.has(t.callId));
  const tools = running.map((t) => {
    const a = args.get(t.callId) ?? {};
    return {
      callId: t.callId,
      name: t.name,
      ...(typeof a.command === "string" ? { command: a.command } : {}),
      ...(typeof a.path === "string" ? { path: a.path } : {}),
    };
  });
  if (slots.some((t) => t.status !== "done")) return { busy: true, phase: "tool", tools, asking };
  const streaming = messageText(live.generation?.message as unknown as Message | undefined);
  return { busy: true, phase: streaming ? "speaking" : "thinking", tools: [], asking: [] };
}

export async function host(init: Init, input: AsyncIterator<Record<string, unknown>>) {
  if (init.v !== V) throw new CmdError("bad_version", `Expected protocol version ${V}, got ${init.v}.`);
  const credentials = new RustCredentialStore();
  const models = buildModels(credentials, init.providers ?? []);
  const { registry, modelTools } = buildRegistry(init);

  let model = init.model;
  mkdirSync(dirname(init.storagePath), { recursive: true });
  const storage = await SqliteStorage.open(openBunSqliteDatabase(init.storagePath));
  const retryDelay = init.settings.retryDelayMs;
  const harness = await Harness.open(
    storage,
    {
      models,
      registry,
      env: ({ cwd }) => executionEnv(init, cwd),
      // One automatic retry: in 1.5 a generation cut off by a crash counts as a failed attempt, so with 0 a
      // crash mid-answer would end the run `interrupted` instead of resending it (plan "Crash retry").
      settings: { retry: { maxRetries: init.settings.maxRetries ?? 1, ...(typeof retryDelay === "number" ? { baseDelayMs: retryDelay } : {}) } },
      identity: { attribution: null },
      onReport: (report) => {
        if (report.kind === "storage_failed") {
          send({ t: "fatal", code: "storage", message: report.error instanceof Error ? report.error.message : String(report.error) });
          void exit(2);
          return;
        }
        const e = report.error;
        process.stderr.write(`buddy-agent: ${report.kind}: ${e instanceof Error ? (e.stack ?? e.message) : String(e)}\n`);
      },
    },
    ctx,
  );

  const conv: Conversation = await harness.root(ctx, { agent: { model: piModel(model), cwd: init.workDir, modelTools } });
  bindRoot(conv);
  const agent = await conv.agent(ctx);
  const want = piModel(model);
  // The stored agent keeps the tools of the sidecar that created it: offer this build's.
  const names = (tools: readonly { name: string }[]) => tools.map((t) => t.name).join(" ");
  const sameModel = agent.model?.provider === want?.provider && agent.model?.modelId === want?.modelId;
  if (!sameModel || names(agent.tools) !== names(modelTools)) await conv.configure({ model: want, modelTools }, ctx);

  // Unfinished work from the previous sidecar: a live task or an input placed in a run.
  const inspection = await harness.inspect(ctx);
  const placed = inspection.submissions.find((s) => String(s.conversationId) === String(conv.id) && s.type === "input" && s.status === "placed");
  const live = inspection.tasks.some((t) => t.state.kind !== "blocked");

  // --- the view → ev records ---------------------------------------------------
  let seq = 0;
  /** Entry ids of the window last seen, to send only new entries. */
  let seen = new Set<string>();
  /** A model-issued call's arguments, from its assistant entry, for the status line. */
  const callArgs = new Map<string, Record<string, unknown>>();
  let lastStatus = "";
  let streamed = "";
  let streamedAttempt = -1;
  let lastLive: LiveState | undefined;

  const noteEntry = (e: Entry) => {
    if (e.kind === INPUT_KIND && e.data?.requestId) {
      const retried = e.data.source === "retry";
      const trigger: Trigger = { requestId: e.data.requestId, source: (retried ? e.data.from : e.data.source) ?? "chat", ...(retried ? { retried: true } : {}) };
      setTrigger(trigger);
    }
    if (e.kind === EVENT_KIND && e.data?.type === "approval_asked" && e.data.callId) markAsked(String(e.data.callId));
    for (const m of e.model ?? []) {
      if (m.role !== "assistant" || typeof m.content === "string") continue;
      for (const p of m.content) if (p.type === "toolCall" && p.id) callArgs.set(p.id, p.arguments ?? {});
    }
    if (callArgs.size > 200) for (const k of [...callArgs.keys()].slice(0, callArgs.size - 200)) callArgs.delete(k);
  };

  const onView = (entries: readonly Entry[], liveDoc: LiveState | undefined, first: boolean) => {
    const next = new Set<string>();
    for (const e of entries) {
      const id = String(e.id);
      next.add(id);
      if (seen.has(id)) continue;
      noteEntry(e);
      if (!first) send({ t: "ev", seq: ++seq, kind: "entry", entry: e });
    }
    seen = next;
    // Streamed answer text: only the new part, or the whole when a new attempt starts over.
    const gen = liveDoc?.run ? liveDoc.generation : undefined;
    const text = messageText(gen?.message as unknown as Message | undefined);
    if (!gen) {
      streamed = "";
      streamedAttempt = -1;
    } else if (text && (gen.attempt !== streamedAttempt || !text.startsWith(streamed))) {
      send({ t: "ev", seq: ++seq, kind: "text", delta: text, reset: true });
      streamed = text;
      streamedAttempt = gen.attempt;
    } else if (text.length > streamed.length) {
      send({ t: "ev", seq: ++seq, kind: "text", delta: text.slice(streamed.length) });
      streamed = text;
    }
    lastLive = liveDoc;
    sendStatus();
  };
  const sendStatus = () => {
    const status: Status = statusOf(lastLive, callArgs);
    // The request the run answers, so the host API knows its `buddy say`s are on time
    // (a resumed run too, whose input entry came before this sidecar).
    const turn = currentTrigger().requestId;
    if (status.busy && turn) status.turn = turn;
    const key = JSON.stringify(status);
    if (key !== lastStatus) {
      lastStatus = key;
      send({ t: "ev", seq: ++seq, kind: "status", status });
    }
  };
  gate.onChange = sendStatus;

  const view = (v: { entries: readonly unknown[]; docs: Readonly<Record<string, unknown>> }) => ({ entries: v.entries as unknown as Entry[], live: v.docs["pi.live"] as LiveState | undefined });
  const watch: ConversationWatch = await conv.watch(ctx);
  {
    const v = view(watch.value);
    onView(v.entries, v.live, true);
  }
  watch.start(async (call) => {
    if (call.kind !== "value") {
      process.stderr.write(`buddy-agent: view failed: ${String(call.error)}\n`);
      return;
    }
    const v = view(call.value);
    onView(v.entries, v.live, false);
  });

  /** Set by `shutdown`: input that lands after it waits for the next sidecar. */
  let closing = false;
  const closingError = () => new CmdError("closing", "desk-buddy's agent is closing.");

  // "clean": the previous sidecar had no run when it stopped; nothing was cut off.
  if ((live || placed) && init.settings.lastExit !== "clean") {
    const cut = currentTrigger();
    if (cut.requestId) setTrigger({ ...cut, resumed: true });
    await recordEvent({ type: "interrupted", by: init.settings.lastExit === "quit" ? "quit" : "crash", ...(placed ? { submissionId: String(placed.id) } : {}) });
  }

  // --- commands --------------------------------------------------------------
  const submitInput = async (requestId: string, text: string, data: Record<string, unknown>, busy?: unknown) => {
    if (!model) throw new CmdError("no_provider", "Choose a model for desk-buddy first.");
    if (closing) throw closingError();
    const existing = await conv.commit((tx) => tx.submissionByRequest(conv.id, requestId), ctx);
    if (existing) return { submissionId: String(existing.id), duplicate: true };
    const sub = await conv.submit(
      { type: "input", content: text, requestId, whenBusy: busy === "steer" ? "steer" : "followUp", entry: { kind: INPUT_KIND, data: { requestId, ...data } as any } },
      ctx,
    );
    return { submissionId: String(sub.id), duplicate: false };
  };

  const handle = async (cmd: Cmd): Promise<unknown> => {
    switch (cmd.op) {
      case "prompt": {
        const requestId = String(cmd.requestId ?? "");
        const source = requestSource(requestId);
        if (!source) throw new CmdError("bad_request", `Not an accepted request id: ${requestId}`);
        if (typeof cmd.text !== "string" || !cmd.text.trim()) throw new CmdError("bad_request", "A prompt needs text.");
        return submitInput(requestId, cmd.text, { source: (cmd.source as string) || source }, cmd.busy);
      }
      case "abort":
        // Closing: try, but ack now so the ack beats the exit; the close ends the run anyway.
        if (closing) void conv.abort(ctx).catch(() => {});
        else await conv.abort(ctx);
        return {};
      case "set_model": {
        model = (cmd.model as ModelRef | null) ?? null;
        await conv.configure({ model: piModel(model), modelTools }, ctx);
        return {};
      }
      case "snapshot": {
        if (closing) throw closingError();
        const v = await conv.context(ctx);
        return { entries: v.entries, status: JSON.parse(lastStatus || "{}") };
      }
      case "credentials_changed":
        credentials.changed(typeof cmd.providerId === "string" ? cmd.providerId : undefined);
        return {};
      case "shutdown":
        closing = true;
        return {};
      default: {
        const fn = command(cmd.op);
        if (!fn) throw new CmdError("unknown_op", `Unknown command: ${cmd.op}`);
        const { t: _t, id: _id, op: _op, ...args } = cmd;
        return fn(args);
      }
    }
  };

  const shutdown = async () => {
    const ms = Number(init.settings.shutdownMs ?? 3000);
    const closed = (async () => {
      await watch.stop().catch(() => {});
      await harness.close(ctx);
    })();
    await Promise.race([closed, new Promise((r) => setTimeout(r, ms))]).catch(() => {});
    await exit(0);
  };

  harness.resume();
  send({ t: "ready", v: V, pid: process.pid, conversationId: String(conv.id) });

  for (;;) {
    const next = await input.next();
    if (next.done) break;
    const rec = next.value;
    if (rec.t === "res") {
      settle(rec as any);
    } else if (rec.t === "cmd") {
      const cmd = rec as Cmd;
      // Commands run concurrently: an abort must not wait behind a prompt's commit.
      void handle(cmd).then(
        (result) => {
          send({ t: "ack", id: cmd.id, ok: true, result: result ?? {} });
          if (cmd.op === "shutdown") void shutdown();
        },
        (e) => {
          const err = e instanceof CmdError ? e : closing ? closingError() : undefined;
          send({ t: "ack", id: cmd.id, ok: false, error: { code: err?.code ?? "error", message: err?.message ?? (e instanceof Error ? e.message : String(e)) } });
        },
      );
    }
  }
  // Rust went away.
  failAll("The app closed.");
  await shutdown();
}
