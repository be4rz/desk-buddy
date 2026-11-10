// buddy.jobs: durable reminders (`buddy remind`, design §4.6). Each reminder
// is a background `buddy.reminder` task on the root conversation that sleeps
// until its time, in agent.sqlite, so it survives a kill -9. Delivery is one
// commit that appends the `reminder_fired` event and ends the task: the
// reminder fires exactly once, however often the sidecar restarts.
// Background: a sleeping reminder never makes the bear busy, and stopping a
// run doesn't cancel it. The live tasks themselves are the list.

import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { type Conversation, defineExtension, defineTask, type Harness, type TaskId } from "@earendil-works/pi-durable";
import { CmdError } from "../../protocol.ts";
import { defineCommand, EVENT_KIND } from "../core/index.ts";

const ctx = BACKGROUND_CONTEXT;
export const REMINDER_KIND = "buddy.reminder";
/** Live reminders at most. */
export const MAX_REMINDERS = 50;
const ID = /^r_[0-9a-f]{12}$/;

export type ReminderInput = { id: string; atMs: number; text: string };
type Phase = { phase: "wait" } | { phase: "deliver" };

export const ReminderTask = defineTask<ReminderInput, Phase, null>({
  name: REMINDER_KIND,
  version: 1,
  initial: () => ({ phase: "wait" }),
  phases: {
    wait: async (task, runtime, context) => {
      await runtime.sleepUntil(task.input.atMs, { phase: "deliver" }, context);
    },
    deliver: async (task, runtime, context) => {
      const root = await runtime.conversation(runtime.conversationId, context);
      if (!root) throw new Error("the reminder's conversation is gone");
      const { id, text, atMs } = task.input;
      await runtime.commit(async (tx) => {
        // The admission comes first: it must precede the commit's table writes.
        await root.submit(tx, { type: "write", entry: { kind: EVENT_KIND, data: { type: "reminder_fired", id, text, atMs, at: runtime.now() } } });
        return { status: "terminal", outcome: { status: "completed", result: null } };
      }, context);
    },
  },
  abort: async (_task, runtime, context) => {
    await runtime.commit(() => ({ status: "terminal", outcome: { status: "aborted", reason: "cancelled" } }), context);
  },
});

export const JobsExt = defineExtension({ name: "buddy.jobs", tasks: [ReminderTask] });

type Live = { taskId: TaskId; input: ReminderInput };

/** The live reminders, soonest first. */
async function live(harness: Harness): Promise<Live[]> {
  const inspection = await harness.inspect(ctx);
  return inspection.tasks
    .filter((t) => t.record.kind === REMINDER_KIND && !t.record.abortRequested)
    .map((t) => ({ taskId: t.record.id, input: t.record.input as ReminderInput }))
    .sort((a, b) => a.input.atMs - b.input.atMs || a.input.id.localeCompare(b.input.id));
}

const view = (r: ReminderInput) => ({ id: r.id, atMs: r.atMs, text: r.text });

/** The `remind.*` commands Rust's host API forwards (ids are minted in Rust). */
export function bindJobs(harness: Harness, root: Conversation) {
  // `reminderId`, since `id` is the command record's own.
  defineCommand("remind.add", async (args) => {
    const id = String(args.reminderId ?? "");
    const atMs = Number(args.atMs);
    const text = typeof args.text === "string" ? args.text.trim() : "";
    if (!ID.test(id)) throw new CmdError("bad_request", "A reminder id looks like r_ and 12 hex digits.");
    if (!Number.isSafeInteger(atMs) || atMs <= 0) throw new CmdError("bad_request", "A reminder needs a time (atMs).");
    // Characters, as Rust counts them (not UTF-16 units).
    if (!text || [...text].length > 500) throw new CmdError("bad_request", "A reminder needs text of 1 to 500 characters.");
    return root.commit(async (tx) => {
      // Idempotent on id, a fired or cancelled one included: a retried add never reminds twice.
      let liveCount = 0;
      let cursor: Parameters<typeof tx.scanTasks>[2];
      do {
        const page = await tx.scanTasks({ kind: REMINDER_KIND, conversationId: root.id }, 200, cursor);
        for (const t of page.items) {
          const input = t.input as ReminderInput;
          if (input.id === id) return { ...view(input), duplicate: true };
          if (t.state.status !== "terminal" && t.state.status !== "completing" && !t.abortRequested) liveCount++;
        }
        cursor = page.items.length < 200 ? undefined : page.next;
      } while (cursor !== undefined);
      if (liveCount >= MAX_REMINDERS) throw new CmdError("too_many", `There are already ${MAX_REMINDERS} reminders; cancel one first.`);
      await tx.createTask(ReminderTask, { id, atMs, text }, { ownership: { kind: "conversation" }, background: true });
      return { id, atMs, text };
    }, ctx);
  });

  defineCommand("remind.list", async () => ({ reminders: (await live(harness)).map((r) => view(r.input)) }));

  defineCommand("remind.cancel", async (args) => {
    const id = String(args.reminderId ?? "");
    const hit = (await live(harness)).find((r) => r.input.id === id);
    if (!hit) return { cancelled: false };
    await harness.abortTask(hit.taskId, ctx);
    return { cancelled: true, id };
  });
}
