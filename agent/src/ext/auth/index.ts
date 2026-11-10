// Sign-in (design §4.7 `auth.login`): pi-ai's `AuthInteraction` mapped 1:1 onto
// `req auth.*` records, which Rust shows in its Sign in… window. The credential
// itself reaches Rust through `cred.modify` (Models.login writes it); nothing
// here keeps, logs or returns a token.

import { type AuthEvent, type AuthInteraction, type AuthPrompt, createProvider, type MutableModels, type OAuthCredential } from "@earendil-works/pi-ai";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { isCancelled, request } from "../../rpc.ts";

/**
 * The interaction for one sign-in; `signal` ends it (the command's abort or the sidecar closing).
 * `refused`: Rust refused the sign-in page (not an https or loopback address); the caller ends
 * the flow with that error instead of waiting on a code for a page nobody opened.
 */
export function rustInteraction(signal?: AbortSignal, refused?: (e: Error) => void): AuthInteraction {
  const notify = (e: AuthEvent) => {
    if (e.type === "auth_url") {
      request("auth.open_url", { url: e.url, ...(e.instructions ? { instructions: e.instructions } : {}) }, signal).catch((err) => {
        if (isCancelled(err, signal)) return;
        refused?.(err instanceof Error ? err : new Error(String(err)));
      });
      return;
    }
    const sent = e.type === "device_code" ? request("auth.device_code", { userCode: e.userCode, verificationUri: e.verificationUri }, signal) : request("auth.progress", { message: e.message }, signal);
    // a code or a progress line that didn't show doesn't end the flow
    sent.catch(() => {});
  };
  const prompt = async (p: AuthPrompt): Promise<string> => {
    const s = p.signal ?? signal;
    if (p.type === "select") {
      const r = await request<{ id: string }>("auth.select", { message: p.message, options: p.options.map((o) => ({ id: o.id, label: o.label })) }, s);
      return String(r.id);
    }
    const r = await request<{ value: string }>("auth.prompt", { message: p.message, ...(p.placeholder ? { placeholder: p.placeholder } : {}) }, s);
    return String(r.value ?? "");
  };
  return { ...(signal ? { signal } : {}), prompt, notify };
}

export const FAKE_OAUTH = "fake-oauth";

/**
 * A scripted OAuth provider for tests (`settings.fakeOAuth`, set only by debug
 * builds): it shows a loopback sign-in page, asks for the code, and accepts
 * only `good`. Its model talks to `baseUrl` (the test's fake model server).
 */
export function addFakeOAuth(models: MutableModels, baseUrl: string) {
  const credential = (n: number): OAuthCredential => ({ type: "oauth", access: `fake-access-${n}`, refresh: `fake-refresh-${n}`, expires: Date.now() + 3_600_000 });
  models.setProvider(
    createProvider({
      id: FAKE_OAUTH,
      name: "Fake OAuth",
      baseUrl,
      auth: {
        oauth: {
          name: "Fake OAuth",
          login: async (interaction) => {
            interaction.notify({ type: "auth_url", url: "http://127.0.0.1:1/fake-oauth/authorize", instructions: "Paste the code the page shows." });
            const code = await interaction.prompt({ type: "manual_code", message: "Paste the code", placeholder: "code", signal: interaction.signal });
            if (code.trim() !== "good") throw new Error("invalid_grant: that code isn't valid");
            interaction.notify({ type: "progress", message: "Signed in." });
            return credential(1);
          },
          refresh: async (c) => ({ ...c, expires: Date.now() + 3_600_000 }),
          toAuth: async (c) => ({ apiKey: c.access }),
        },
      },
      models: [
        {
          id: "fake-1",
          name: "Fake 1",
          api: "openai-completions",
          provider: FAKE_OAUTH,
          baseUrl,
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 128000,
          maxTokens: 4096,
          compat: { supportsStore: false, supportsDeveloperRole: false },
        },
      ] as any,
      api: openAICompletionsApi(),
    }),
  );
}
