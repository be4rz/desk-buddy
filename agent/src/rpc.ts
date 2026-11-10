// Sidecar → Rust requests (design §2.4): `req` out, `res` back by id. An
// aborted wait sends `cancel` so Rust frees whatever was waiting.

import { send, type Res } from "./protocol.ts";

export class RpcError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
  }
}

/** Whether `e` ended a wait because it was stopped: our own abort, or Rust's "cancelled" reply. */
export function isCancelled(e: unknown, signal?: AbortSignal): boolean {
  return !!signal?.aborted || (e instanceof RpcError && e.code === "cancelled");
}

type Waiter = { resolve: (v: unknown) => void; reject: (e: unknown) => void };

const waiting = new Map<string, Waiter>();
let next = 0;

export function request<T = unknown>(op: string, args: Record<string, unknown> = {}, signal?: AbortSignal): Promise<T> {
  if (signal?.aborted) return Promise.reject(new RpcError("cancelled", "Stopped."));
  const id = `r-${++next}`;
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => {
      if (!waiting.delete(id)) return;
      send({ t: "cancel", id });
      reject(new RpcError("cancelled", "Stopped."));
    };
    waiting.set(id, {
      resolve: (v) => {
        signal?.removeEventListener("abort", onAbort);
        resolve(v as T);
      },
      reject: (e) => {
        signal?.removeEventListener("abort", onAbort);
        reject(e);
      },
    });
    signal?.addEventListener("abort", onAbort, { once: true });
    send({ t: "req", id, op, ...args });
  });
}

export function settle(res: Res) {
  const w = waiting.get(res.id);
  if (!w) return;
  waiting.delete(res.id);
  if (res.ok) w.resolve(res.result);
  else w.reject(new RpcError(res.error?.code ?? "error", res.error?.message ?? "Request failed."));
}

/** Rust went away: nothing will answer. */
export function failAll(message: string) {
  for (const [id, w] of waiting) {
    waiting.delete(id);
    w.reject(new RpcError("closed", message));
  }
}
