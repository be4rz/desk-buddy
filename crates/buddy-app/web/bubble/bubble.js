// The speech bubble page (design §4.8). The app sends `bubble://show` (a BubbleModel) and
// `bubble://hide`; the page answers with prompt_submit, card_answer, bubble_dismiss, bubble_size,
// bubble_retry, bubble_recall, bubble_reply and reminder_snooze. Every piece of text goes in with
// textContent or a text node (never HTML): the small markdown subset below builds elements itself.
"use strict";
(function () {
  const T = window.__TAURI__;
  const root = document.getElementById("root");
  const invoke = (cmd, args) => T.core.invoke(cmd, args || {}).catch((e) => console.error(cmd, e));
  let model = null;
  /** What the person typed and didn't send (Esc keeps it for the next time the prompt opens). */
  let draft = "";
  /** Sent prompts, oldest first (↑ / ↓ in an empty prompt), for this run of the app only. */
  const history = [];

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function btn(label, cls, onclick) {
    const b = el("button", "btn" + (cls ? " " + cls : ""), label);
    b.type = "button";
    b.addEventListener("click", onclick);
    return b;
  }
  function head(left, ...right) {
    const m = el("div", "head");
    const r = el("div", "head-r");
    r.append(...right.filter(Boolean));
    m.append(left || el("span"), r);
    return m;
  }
  function close(label) {
    const x = el("button", "x", "✕");
    x.type = "button";
    x.title = label || "Close (Esc)";
    x.setAttribute("aria-label", label || "Close");
    x.addEventListener("click", () => invoke("bubble_dismiss"));
    return x;
  }
  function pill(text, cls) {
    return el("span", "pill" + (cls ? " " + cls : ""), text);
  }
  function row(...kids) {
    const r = el("div", "row");
    r.append(...kids.filter(Boolean));
    return r;
  }

  // ---------- markdown (a safe subset: paragraphs, lists, code, bold, italic, links as text) ----------

  const INLINE = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\*[^*\s][^*\n]*\*)|(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))/g;
  function inline(parent, text) {
    let at = 0;
    for (const m of text.matchAll(INLINE)) {
      if (m.index > at) parent.append(text.slice(at, m.index));
      const t = m[0];
      if (m[1]) parent.append(el("code", null, t.slice(1, -1)));
      else if (m[2]) parent.append(el("b", null, t.slice(2, -2)));
      else if (m[3]) parent.append(el("i", null, t.slice(1, -1)));
      else {
        // a link is shown as its text and its address: nothing in the bubble navigates
        const cut = t.indexOf("](");
        parent.append(t.slice(1, cut), el("span", "url", " (" + t.slice(cut + 2, -1) + ")"));
      }
      at = m.index + t.length;
    }
    if (at < text.length) parent.append(text.slice(at));
  }
  function markdown(text) {
    const out = document.createDocumentFragment();
    const lines = text.split("\n");
    let para = null, list = null;
    const flush = () => { para = null; list = null; };
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (/^\s*```/.test(line)) {
        // a code block runs to its closing fence, or to the end while the answer still streams
        const body = [];
        for (i++; i < lines.length && !/^\s*```/.test(lines[i]); i++) body.push(lines[i]);
        flush();
        out.append(el("pre", "code", body.join("\n")));
        continue;
      }
      if (!line.trim()) { flush(); continue; }
      const h = /^#{1,6}\s+(.*)$/.exec(line);
      if (h) {
        flush();
        inline(out.appendChild(el("div", "h")), h[1]);
        continue;
      }
      const li = /^\s*(?:([-*•])|(\d+)[.)])\s+(.*)$/.exec(line);
      if (li) {
        const tag = li[1] ? "ul" : "ol";
        if (!list || list.tagName.toLowerCase() !== tag) {
          para = null;
          list = out.appendChild(el(tag));
          if (li[2]) list.start = Number(li[2]);
        }
        inline(list.appendChild(el("li")), li[3]);
        continue;
      }
      if (!para || list) { list = null; para = out.appendChild(el("p")); }
      else para.append("\n");
      inline(para, line);
    }
    return out;
  }

  /** The answer text: scrolls after ~11 lines, with a fade where there is more below. */
  function textBox(text, md) {
    const box = el("div", "text");
    setText(box, text, md);
    box.addEventListener("scroll", () => fade(box));
    return box;
  }
  function setText(box, text, md) {
    if (md) box.replaceChildren(markdown(text || ""));
    else box.textContent = text || "";
  }
  function fade(box) {
    box.classList.toggle("more", box.scrollHeight - box.scrollTop - box.clientHeight > 4);
  }

  // ---------- the modes ----------

  function prompt(b, m) {
    const recall = m.recall && btn("Last answer", "ghost", () => { keep(); invoke("bubble_recall"); });
    if (recall) recall.title = "Show my last answer again";
    b.append(head(el("span", "title", "Ask me anything"), recall, close()));
    const ta = el("textarea");
    ta.rows = 2;
    ta.setAttribute("aria-label", "Your question");
    ta.placeholder = "A question, or “remind me to stretch in 20 min”";
    ta.value = draft;
    const ask = btn(m.busy ? "Ask instead" : "Ask", "primary", send);
    let at = history.length; // where ↑ / ↓ is in the history; history.length = the draft
    function keep() { draft = ta.value; }
    function grow() {
      ta.style.height = "auto";
      ta.style.height = Math.min(ta.scrollHeight + 3, 150) + "px";
      ask.disabled = !ta.value.trim();
    }
    function send() {
      const t = ta.value.trim();
      if (!t) return;
      if (history[history.length - 1] !== t) history.push(t);
      if (history.length > 30) history.shift();
      draft = "";
      invoke("prompt_submit", { text: t });
    }
    function recallPrompt(step) {
      const to = at + step;
      if (to < 0 || to > history.length) return false;
      if (at === history.length) draft = ta.value;
      at = to;
      ta.value = at === history.length ? draft : history[at];
      grow();
      ta.setSelectionRange(ta.value.length, ta.value.length);
      return true;
    }
    ta.addEventListener("input", () => { at = history.length; keep(); grow(); });
    ta.addEventListener("keydown", (e) => {
      if (e.isComposing) return;
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
      // ↑ on the first line / ↓ on the last one walk through what was asked before
      else if (e.key === "ArrowUp" && !ta.value.slice(0, ta.selectionStart).includes("\n") && (at < history.length || !ta.value)) { if (recallPrompt(-1)) e.preventDefault(); }
      else if (e.key === "ArrowDown" && at < history.length && !ta.value.slice(ta.selectionEnd).includes("\n")) { if (recallPrompt(1)) e.preventDefault(); }
    });
    b.append(ta);
    if (m.busy) b.append(el("div", "note info", "I’m still on your last question. Asking now replaces it."));
    b.append(row(el("span", "hint", "⏎ send · ⇧⏎ new line" + (history.length ? " · ↑ earlier" : "")), ask));
    setTimeout(() => { grow(); ta.focus(); ta.setSelectionRange(ta.value.length, ta.value.length); }, 0);
  }

  function statusText(m) {
    return m.activity || (m.text ? "Answering" : "Thinking");
  }

  function speech(b, m) {
    const text = textBox(m.text, true);
    if (m.streaming) {
      const st = el("span", "status");
      st.setAttribute("role", "status");
      st.append(el("span", "status-text", statusText(m)), el("span", "dots"));
      const stop = btn("Stop", "ghost stop", () => invoke("bubble_dismiss"));
      stop.title = "Stop answering (Esc)";
      b.append(head(st, stop));
      text.classList.toggle("empty", !m.text);
      b.append(text);
      requestAnimationFrame(() => { text.scrollTop = text.scrollHeight; fade(text); });
      return;
    }
    b.classList.add("plain");
    b.append(close(), text);
    if (m.reply) {
      const copy = btn("Copy", "ghost", () => copyText(m.text || "", copy));
      b.append(row(el("span", "hint"), copy, btn("Reply", "ghost", () => invoke("bubble_reply"))));
    }
    requestAnimationFrame(() => fade(text));
  }

  /** A streaming answer grows in place: the Stop button, the selection and the scroll stay put. */
  function patchStreaming(m) {
    const b = root.firstElementChild;
    const text = b.querySelector(".text");
    const stuck = text.scrollHeight - text.scrollTop - text.clientHeight < 24;
    b.querySelector(".status-text").textContent = statusText(m);
    setText(text, m.text, true);
    text.classList.toggle("empty", !m.text);
    if (stuck) text.scrollTop = text.scrollHeight;
    fade(text);
  }

  function copyText(t, b) {
    const done = () => { b.textContent = "Copied"; setTimeout(() => { b.textContent = "Copy"; }, 1500); };
    const fallback = () => {
      const ta = el("textarea");
      ta.value = t;
      ta.style.cssText = "position:fixed;opacity:0;top:0;left:0;height:1px";
      document.body.append(ta);
      ta.select();
      try { if (document.execCommand("copy")) done(); } catch (_) { /* the text is selectable by hand */ }
      ta.remove();
    };
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(t).then(done, fallback);
    else fallback();
  }

  function setup(b) {
    b.append(head(el("span", "title", "Hi, I’m your desk buddy!"), close("Not now")));
    b.append(el("div", "text", "Connect an AI account and I can answer questions, keep reminders and lend a paw. Your key stays in your Mac’s keychain."));
    b.append(row(btn("Not now", "", () => invoke("bubble_dismiss")), btn("Set up…", "primary", () => invoke("bubble_retry"))));
  }

  function card(b, m) {
    const c = m.card || {};
    const tainted = !!c.taintNote;
    const total = c.total || 1;
    b.append(head(pill("Needs your OK", tainted ? "rose" : ""), total > 1 ? el("span", "count", (c.index || 1) + " of " + total) : null));
    b.append(el("div", "card-title", c.title || "Allow this?"));
    if (c.detail) b.append(el("div", "text", c.detail));
    if (c.command) b.append(el("div", "cmd", c.command));
    if (tainted) b.append(el("div", "note taint", c.taintNote));
    const answer = (a) => () => invoke("card_answer", { callId: c.callId, answer: a });
    const always = c.canAlways && btn("Always allow", "", answer("allow_always"));
    if (always) always.title = "Allow this, and don’t ask again for commands like it";
    b.append(row(btn("Deny", "danger", answer("deny")), el("span", "gap"), always, btn("Allow once", "primary", answer("allow"))));
  }

  function reminder(b, m) {
    // "set N min ago" only when the app knows (reminders stored before p4 don't say)
    const ago = typeof m.setMinAgo !== "number" ? "" : m.setMinAgo < 1 ? " · set just now" : m.setMinAgo < 90 ? " · set " + m.setMinAgo + " min ago" : " · set " + Math.round(m.setMinAgo / 60) + " h ago";
    b.append(head(pill("Reminder" + ago, "teal"), close("Done")));
    b.append(el("div", "text big", m.text || ""));
    const snooze = (min) => () => invoke("reminder_snooze", { id: m.id || "", minutes: min });
    b.append(row(el("span", "hint", "Snooze for"), btn("10 min", "", snooze(10)), btn("1 hour", "", snooze(60)), btn("Done", "primary", () => invoke("bubble_dismiss"))));
  }

  /** What went wrong, in plain words; the service's own message goes underneath. */
  function errorTitle(e) {
    const msg = (e.message || "").toLowerCase();
    switch (e.reason) {
      case "keychain": return "I can’t read my saved key from the keychain.";
      case "no_model": return "I’m not connected to an AI account yet.";
      case "no_agent": case "agent_stopped": return "My helper process isn’t running.";
      case "interrupted": case "aborted": return "That one got interrupted.";
    }
    if (/\b(401|403)\b|unauthori|invalid.*key|authentication/.test(msg)) return "The AI service didn’t accept my key.";
    if (/\b429\b|rate.?limit|quota/.test(msg)) return "I’ve hit the AI service’s rate limit.";
    if (/\b(529|503|502)\b|overloaded|unavailable/.test(msg)) return "The AI service is busy right now.";
    if (/network|timed? ?out|dns|connect|offline|fetch failed/.test(msg)) return "I couldn’t reach the AI service.";
    return e.action === "sign_in" ? "I can’t answer until an AI account is connected." : "Something went wrong, so I couldn’t answer.";
  }

  function error(b, m) {
    const e = m.error || {};
    const signIn = e.action === "sign_in";
    b.append(head(pill("Couldn’t answer", "rose"), close()));
    b.append(el("div", "text", errorTitle(e)));
    if (e.message) b.append(el("div", "note err", e.message));
    // an unprompted turn's error has no action: Try again would re-send the person's last prompt
    const act = (signIn || e.action === "retry") && btn(signIn ? "Open Accounts…" : "Try again", "primary", () => invoke("bubble_retry"));
    b.append(row(btn("Close", "", () => invoke("bubble_dismiss")), act));
  }

  const modes = { prompt, speech, card, reminder, error, setup };

  function render(m) {
    const was = model;
    model = m;
    if (was && was.mode === "speech" && was.streaming && m.mode === "speech" && m.streaming && root.firstElementChild) {
      patchStreaming(m);
      return report(true);
    }
    root.replaceChildren();
    const b = el("div", "bubble");
    b.dataset.mode = m.mode;
    // a card and a reminder interrupt: they are announced; a streaming answer is not read out word by word
    if (m.mode === "card" || m.mode === "error") b.setAttribute("role", "alertdialog");
    else if (m.mode !== "prompt" && !m.streaming) b.setAttribute("role", "status");
    (modes[m.mode] || speech)(b, m);
    root.append(b);
    report(true); // the app shows a new model once it knows its size
  }

  function clear() {
    model = null;
    root.replaceChildren();
    last = "";
  }

  let last = "";
  // Measured right away (reading the rect lays the page out), not in a frame callback: the window
  // is hidden until the app has this size, and a hidden page may get no animation frames.
  function report(force) {
    const r = root.getBoundingClientRect();
    const w = Math.ceil(r.width) || 306, h = Math.ceil(r.height) || 1;
    if (!force && w + "x" + h === last) return;
    last = w + "x" + h;
    invoke("bubble_size", { w, h });
  }

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && model && model.mode !== "card") { e.preventDefault(); invoke("bubble_dismiss"); }
  });
  new ResizeObserver(() => { if (model) report(); }).observe(root);

  T.event.listen("bubble://show", (e) => render(e.payload));
  T.event.listen("bubble://hide", () => clear());
  report(true); // loaded: the app resends what's shown
})();
