// buddy.core: the step limit, buddy events (recordEvent), extension commands
// (defineCommand) and the prefix sections. Ported from 2market's twomarket.core.

import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { type Conversation, defineExtension, GenerationTask, hook, ToolTask } from "@earendil-works/pi-durable";
import type { Init } from "../../protocol.ts";
import { sections } from "./prompt.ts";

/** The custom entry kind every buddy event is committed as. */
export const EVENT_KIND = "buddy.event";
/** The entry kind a prompt's input is placed as: the model still sees the user message. */
export const INPUT_KIND = "buddy.input";

const ctx = BACKGROUND_CONTEXT;
let root: Conversation | undefined;

export function bindRoot(conversation: Conversation) {
  root = conversation;
}

export function rootId(): string | undefined {
  return root ? String(root.id) : undefined;
}

/**
 * Commit a buddy event to the root conversation before it is shown, so it
 * reaches the view in order. A write of only kind and data is appended at
 * once, even mid-run.
 */
export async function recordEvent(event: { type: string; [k: string]: unknown }): Promise<void> {
  if (!root) throw new Error("recordEvent before the root conversation is open");
  await root.submit({ type: "write", entry: { kind: EVENT_KIND, data: { at: Date.now(), ...event } as any } }, ctx);
}

/** From a hook: no awaiting the commit inside the task's own step. */
export function recordEventLater(event: { type: string; [k: string]: unknown }) {
  recordEvent(event).catch((e) => process.stderr.write(`buddy-agent: could not record ${event.type}: ${e}\n`));
}

export type CommandFn = (args: Record<string, unknown>) => Promise<unknown>;
const commands = new Map<string, CommandFn>();

/** An extension command: a `cmd` op no built-in handles. */
export function defineCommand(op: string, fn: CommandFn) {
  commands.set(op, fn);
}

export function command(op: string): CommandFn | undefined {
  return commands.get(op);
}

/** The model round in progress since the newest user message, per conversation (1 = the first request). */
const rounds = new Map<string, number>();
/** Rounds whose step_limit was recorded, so a replayed hook doesn't record it twice. */
const limited = new Set<string>();

export function createCore(init: Init) {
  const limit = init.settings.stepLimit ?? 50;
  const over = (conversationId: unknown) => (rounds.get(String(conversationId)) ?? 0) >= limit;
  return defineExtension({
    name: "buddy.core",
    sections: sections(init),
    hooks: [
      hook(GenerationTask, {
        beforeRequest: ({ messages }, api) => {
          const lastUser = messages.map((m) => m.role).lastIndexOf("user");
          rounds.set(String(api.conversationId), messages.slice(lastUser).filter((m) => m.role === "assistant").length + 1);
          return undefined;
        },
        afterTools: (assistant, _results, api) => {
          if (!over(api.conversationId) || String(api.conversationId) !== String(root?.id) || limited.has(String(assistant))) return;
          limited.add(String(assistant));
          recordEventLater({ type: "step_limit", rounds: rounds.get(String(api.conversationId)) });
        },
      }),
      hook(ToolTask, {
        // Every result of the round ends the run: no further model request.
        afterTool: (_call, result, api) => (over(api.conversationId) ? { ...result, control: { ...result.control, terminate: true } } : undefined),
      }),
    ],
  });
}
