// pi-ai's CredentialStore over Rust's OS credential store (design §2.4). Keys
// and tokens live only in this process's memory: never a file, an environment
// variable or the shell.

import type { AuthOperationOptions, Credential, CredentialInfo, CredentialStore } from "@earendil-works/pi-ai";
import { request } from "./rpc.ts";

export class RustCredentialStore implements CredentialStore {
  /** Reads are kept for the sidecar's life, until Rust says they changed. */
  private cache = new Map<string, Credential | null>();

  async read(providerId: string, options?: AuthOperationOptions): Promise<Credential | undefined> {
    if (!this.cache.has(providerId)) {
      const c = await request<Credential | null>("cred.read", { providerId }, options?.signal);
      this.cache.set(providerId, c ?? null);
    }
    return this.cache.get(providerId) ?? undefined;
  }

  list(options?: AuthOperationOptions): Promise<readonly CredentialInfo[]> {
    return request<CredentialInfo[]>("cred.list", {}, options?.signal);
  }

  /** Rust holds a lease per provider across every sidecar, so one refresh writes once. */
  async modify(providerId: string, fn: (current: Credential | undefined) => Promise<Credential | undefined>, options?: AuthOperationOptions): Promise<Credential | undefined> {
    const { lease, credential } = await request<{ lease: string; credential: Credential | null }>("cred.modify_begin", { providerId }, options?.signal);
    let next: Credential | undefined;
    try {
      next = await fn(credential ?? undefined);
    } catch (e) {
      await request("cred.modify_end", { lease, write: false }).catch(() => {});
      throw e;
    }
    const done = await request<{ credential: Credential | null }>("cred.modify_end", next ? { lease, write: true, credential: next } : { lease, write: false });
    this.cache.set(providerId, done.credential ?? null);
    return done.credential ?? undefined;
  }

  async delete(providerId: string, options?: AuthOperationOptions): Promise<void> {
    await request("cred.delete", { providerId }, options?.signal);
    this.cache.set(providerId, null);
  }

  changed(providerId?: string) {
    if (providerId) this.cache.delete(providerId);
    else this.cache.clear();
  }
}
