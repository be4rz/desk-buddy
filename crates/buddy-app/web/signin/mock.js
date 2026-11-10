// Browser-only parity mock: `index.html?mock=<state>` installs a fake `__TAURI__` and shows one of
// the prototype's Sign in… states (`#signin`, `#signin-error`, `#signin-done`, plus `signin-code`
// and `signin-empty`). Inside the app (`__TAURI__` present) or without ?mock it does nothing.
// Commands the page invokes are recorded in `window.__calls`.
"use strict";
(function () {
  const state = new URLSearchParams(location.search).get("mock");
  if (window.__TAURI__ || !state) return;
  const P = (id, name, detail, methods, status, model) => ({ id, name, detail, methods, status: status || "none", model: model || null });
  const anthropic = (st, m) => P("anthropic", "Anthropic", "Claude account (OAuth) or API key", ["oauth", "api_key"], st, m);
  const openai = P("openai", "OpenAI / ChatGPT", "ChatGPT account (OAuth) or API key", ["oauth", "api_key"]);
  const copilot = P("github-copilot", "GitHub Copilot", "device code", ["oauth"]);
  const openrouter = (st) => P("openrouter", "OpenRouter", "API key", ["api_key"], st);
  const rest = [P("google", "Google Gemini", "API key", ["api_key"]), P("xai", "xAI", "Grok subscription (OAuth) or API key", ["oauth", "api_key"]), P("mistral", "Mistral", "API key", ["api_key"])];
  const flow = (o) => ({ providerId: "anthropic", name: "Anthropic", phase: "starting", url: null, instructions: null, message: null, placeholder: null, userCode: null, verificationUri: null, options: [], progress: null, error: null, ...o });
  const states = {
    // #signin: the list, a device code on screen, the key form open (opened by clicking Add key)
    signin: { providers: [anthropic(), openai, copilot, openrouter(), ...rest], flow: flow({ providerId: "github-copilot", name: "GitHub Copilot", phase: "device", userCode: "WDJB-MJHT", verificationUri: "https://github.com/login/device" }), keychain: null, canOAuth: true, notice: null, openKey: "openrouter" },
    "signin-code": { providers: [anthropic(), openai, copilot, openrouter(), ...rest], flow: flow({ phase: "code", url: "https://claude.ai/oauth/authorize?code=true&client_id=example", instructions: "Sign in, then paste the code the page shows.", message: "Paste the code shown after sign-in", placeholder: "code#state" }), keychain: null, canOAuth: true, notice: null },
    // #signin-error: a wrong code, and a keychain read error
    "signin-error": { providers: [anthropic(), openai, copilot, openrouter(), ...rest], flow: flow({ phase: "error", url: "https://claude.ai/oauth/authorize?code=true&client_id=example", instructions: "Sign in, then paste the code the page shows.", error: "That code didn't work (invalid_grant). Start again to get a fresh one." }), keychain: "access denied", canOAuth: true, notice: null },
    // #signin-done: signed in + a key saved, and the list's empty state
    "signin-done": { providers: [anthropic("signed_in", "claude-sonnet-5-5"), openrouter("key_saved")], flow: null, keychain: null, canOAuth: true, notice: { ok: true, text: "Signed in to Anthropic." } },
    "signin-signed-out": { providers: [anthropic(), openai, copilot, openrouter(), ...rest], flow: null, keychain: null, canOAuth: false, notice: { ok: false, text: "Couldn't save the key: the keychain refused it." } },
  };
  const listeners = {};
  window.__calls = [];
  window.__TAURI__ = {
    core: { invoke: (cmd, args) => { window.__calls.push([cmd, args]); return Promise.resolve(cmd === "signin_state" ? null : null); } },
    event: { listen: (name, cb) => { listeners[name] = cb; return Promise.resolve(() => {}); } },
  };
  window.__mockState = (s) => listeners["signin://state"] && listeners["signin://state"]({ payload: s });
  addEventListener("DOMContentLoaded", () => setTimeout(() => {
    const s = states[state];
    if (!s) return;
    window.__mockState(s);
    if (s.openKey) {
      const b = [...document.querySelectorAll('[data-provider="' + s.openKey + '"] button')].find((x) => x.textContent === "Add key");
      if (b) b.click();
      const f = document.querySelector('input[type="password"]');
      if (f) { f.value = "sk-or-v1-xxxxxxxxxxxxxxxxxxxx"; f.dispatchEvent(new Event("input")); }
    }
  }, 0));
})();
