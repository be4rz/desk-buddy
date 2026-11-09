// buddy.gate: every tool call, a nested one included, asks Rust's policy
// before it runs (desk-buddy plan §2 "Permissions"). Installed last, so it
// decides on the final arguments. Any error, timeout or throw blocks the call:
// fail closed. Ported from 2market's twomarket.gate.

import { defineExtension, hook, ToolTask } from "@earendil-works/pi-durable";
import { request } from "../../rpc.ts";
import { recordEventLater as record, rootId } from "../core/index.ts";

export type Decision = { decision: "allow" } | { decision: "deny"; reason?: string } | { decision: "ask"; approval: Record<string, unknown> };
export type Waited = { answer: string; decision: "allow" | "deny"; reason?: string; rule?: unknown; taint?: unknown };

/** How long `policy.decide` may take before the call is blocked; `settings.decideMs` overrides it. */
let decideMs = 30_000;
export function setDecideMs(ms: number | undefined) {
  if (typeof ms === "number" && ms > 0) decideMs = ms;
}

/** The input whose run is making calls. `resumed`: the run a crashed sidecar left cut off. */
export type Trigger = { requestId: string; source: string; retried?: boolean; resumed?: boolean };

/** The host sets it as inputs are placed; `policy.decide` and `BUDDY_TURN` read it. */
let trigger: Trigger = { requestId: "", source: "chat" };
export function setTrigger(t: Trigger) {
  trigger = t;
}
export function currentTrigger(): Trigger {
  return trigger;
}

/** Approvals already shown, so a replayed hook doesn't show a card twice. The host seeds it from the conversation on open. */
const asked = new Set<string>();
export function markAsked(callId: string) {
  asked.add(callId);
}

/**
 * Calls the gate let through, and calls waiting on the person. The status line shows a running call only once it is
 * cleared, so the mascot never looks busy with a call that is still being decided. `onChange` refreshes it.
 */
export const gate = { cleared: new Set<string>(), asking: new Set<string>(), onChange: () => {} };

function timeout(ms: number, signal?: AbortSignal) {
  const c = new AbortController();
  const t = setTimeout(() => c.abort(), ms);
  signal?.addEventListener("abort", () => c.abort(), { once: true });
  return { signal: c.signal, done: () => clearTimeout(t) };
}

export const GateExt = defineExtension({
  name: "buddy.gate",
  hooks: [
    hook(ToolTask, {
      beforeTool: async (call, api, context) => {
        const signal = (context as { abortSignal?: AbortSignal }).abortSignal;
        const child = String(api.conversationId) !== rootId();
        let d: Decision;
        const t = timeout(decideMs, signal);
        try {
          d = await request<Decision>(
            "policy.decide",
            {
              callId: call.id,
              tool: call.name,
              args: call.arguments,
              conversation: child ? "child" : "root",
              depth: child ? 1 : 0,
              ...(call.parent ? { nested: true } : {}),
              trigger,
            },
            t.signal,
          );
        } catch (e) {
          const why = t.signal.aborted && !signal?.aborted ? `no answer within ${decideMs >= 1000 ? `${Math.round(decideMs / 1000)} s` : `${decideMs} ms`}` : e instanceof Error ? e.message : String(e);
          return { block: `The permission check failed (${why}), so it didn't run.` };
        } finally {
          t.done();
        }
        if (d?.decision === "allow") {
          gate.cleared.add(call.id);
          gate.onChange();
          return undefined;
        }
        if (d?.decision === "deny") return { block: d.reason || "The permissions don't allow it." };
        if (d?.decision !== "ask") return { block: "The permissions don't allow it." };
        if (!asked.has(call.id)) {
          asked.add(call.id);
          record({ type: "approval_asked", callId: call.id, ...d.approval });
        }
        let w: Waited;
        gate.asking.add(call.id);
        gate.onChange();
        try {
          w = await request<Waited>("policy.wait", { callId: call.id }, signal);
        } catch (e) {
          return { block: `The answer didn't come back (${e instanceof Error ? e.message : String(e)}), so it didn't run.` };
        } finally {
          gate.asking.delete(call.id);
          if (w! && w.decision === "allow") gate.cleared.add(call.id);
          gate.onChange();
        }
        record({ type: "approval_answered", callId: call.id, answer: w.answer, ...(w.rule ? { rule: w.rule } : {}) });
        if (w.taint) record({ type: "taint_set", ...(w.taint as object) });
        return w.decision === "allow" ? undefined : { block: w.reason || "The person declined this call." };
      },
    }),
  ],
});
