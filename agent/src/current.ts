// The model the agent runs with, for the parts that size themselves by it:
// compaction thresholds, clearing and whether `look` is offered. Children
// start as a copy of the root's agent, so one current model serves both.

import type { Models } from "@earendil-works/pi-ai";
import type { ModelRef } from "./protocol.ts";

let models: Models | undefined;
let current: ModelRef | null = null;

export function setModels(m: Models) {
  models = m;
}

export function setCurrentModel(m: ModelRef | null) {
  current = m;
}

/** The models collection and current model ref, for a direct one-shot model call (`memory.extract`). */
export function currentModelCall(): { models: Models; ref: ModelRef } | undefined {
  return models && current ? { models, ref: current } : undefined;
}

/** pi-ai's record of a model, when the catalog has it. */
export function modelInfo(m: ModelRef | null = current): { contextWindow: number; input: readonly string[] } | undefined {
  if (!m || !models) return undefined;
  const found = models.getModel(m.provider, m.model) as { contextWindow?: number; input?: readonly string[] } | undefined;
  return found ? { contextWindow: found.contextWindow || 128000, input: found.input ?? ["text"] } : undefined;
}

/** The current model's context window in tokens (128 000 when unknown). */
export function contextWindow(): number {
  return modelInfo()?.contextWindow ?? 128000;
}

export function takesImages(m: ModelRef | null = current): boolean {
  return modelInfo(m)?.input.includes("image") ?? false;
}
