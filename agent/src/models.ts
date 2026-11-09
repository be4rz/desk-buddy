// pi-ai models over Rust's credentials: every built-in provider, plus the
// endpoints init lists (local servers, custom endpoints, debug base overrides),
// which replace a built-in of the same id.

import { type ApiKeyAuth, createProvider, type MutableModels } from "@earendil-works/pi-ai";
import { anthropicMessagesApi } from "@earendil-works/pi-ai/api/anthropic-messages.lazy";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import { openAIResponsesApi } from "@earendil-works/pi-ai/api/openai-responses.lazy";
import { builtinModels } from "@earendil-works/pi-ai/providers/all";
import type { RustCredentialStore } from "./credentials.ts";
import type { ProviderDef } from "./protocol.ts";

/** A key from the credential store only: never an environment variable. A local server needs none. */
function storedKeyAuth(name: string, keyless: boolean): ApiKeyAuth {
  return {
    name,
    resolve: async ({ credential }) => {
      const key = credential?.key;
      if (key) return { auth: { apiKey: key } };
      return keyless ? { auth: { apiKey: "local" } } : undefined;
    },
  };
}

const API = {
  "openai-completions": openAICompletionsApi,
  "openai-responses": openAIResponsesApi,
  "anthropic-messages": anthropicMessagesApi,
};

function isLocal(baseUrl: string) {
  try {
    const h = new URL(baseUrl).hostname;
    return h === "localhost" || h === "127.0.0.1" || h === "[::1]" || h === "::1";
  } catch {
    return false;
  }
}

export function buildModels(credentials: RustCredentialStore, defs: ProviderDef[]): MutableModels {
  const models = builtinModels({ credentials });
  for (const d of defs) {
    const api = API[d.api];
    if (!api) continue;
    models.setProvider(
      createProvider({
        id: d.id,
        name: d.name,
        baseUrl: d.baseUrl,
        // Keep the built-in's sign-in: an endpoint override must not drop it.
        auth: { apiKey: storedKeyAuth(`${d.name} key`, isLocal(d.baseUrl)), oauth: models.getProvider(d.id)?.auth.oauth },
        models: d.models.map((m) => ({
          id: m.id,
          name: m.name,
          api: d.api,
          provider: d.id,
          baseUrl: d.baseUrl,
          reasoning: m.reasoning,
          input: m.input,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: m.contextWindow || 128000,
          maxTokens: m.maxTokens || 8192,
          ...(d.api === "openai-completions" || m.compat ? { compat: { ...(d.api === "openai-completions" ? { supportsStore: false, supportsDeveloperRole: false } : {}), ...m.compat } } : {}),
        })) as any,
        api: api(),
      }),
    );
  }
  return models;
}
