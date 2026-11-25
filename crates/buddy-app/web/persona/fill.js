// The persona fill rule, the page's copy of `mascot_core::persona::fill` (Rust is authoritative). Both
// run the cases of crates/mascot-core/tests/fixtures/persona_fill.json (the Rust unit test and
// agent/test/unit/persona_fill.test.ts), so the sample bubble reads exactly like the bear's own lines.
//   {me}          the effective name
//   {you}         what it calls the person; unset, the token goes with the spaces and one comma before it
//   {catchphrase} capitalised at the start of the text or after . ! ? and a space; a "." right after it is
//                 kept only when the catchphrase doesn't end in . ! ?; unset, the token, its "." and the
//                 space before it go
// Any other {x} stays as written, and a filled value is never scanned again.
// Loaded as a plain script (window.PersonaFill) or evaluated with a `module` object (the bun test).
"use strict";
(function (root) {
  const KNOWN = ["me", "you", "catchphrase"];
  const isWs = (c) => /\s/.test(c);
  const trimEnd = (s) => s.replace(/\s+$/u, "");

  /** `text` with {me}, {you} and {catchphrase} filled from `f` = {me, you, catchphrase} (null/"" = unset). */
  function fill(text, f) {
    const you = f.you || null, cp = f.catchphrase || null;
    let out = "";
    let rest = String(text);
    for (;;) {
      const i = rest.indexOf("{");
      if (i < 0) break;
      out += rest.slice(0, i);
      const after = rest.slice(i + 1);
      const j = after.indexOf("}");
      const name = j > 0 ? after.slice(0, j) : null;
      if (name === null || !KNOWN.includes(name)) {
        out += "{";
        rest = after;
        continue;
      }
      rest = after.slice(j + 1);
      if (name === "me") out += f.me || "";
      else if (name === "you") {
        if (you) out += you;
        else {
          out = trimEnd(out);
          if (out.endsWith(",")) out = out.slice(0, -1);
        }
      } else {
        const dot = rest.startsWith(".");
        if (dot) rest = rest.slice(1);
        if (cp) {
          const before = trimEnd(out);
          const start = before === "" || (before.length < out.length && /[.!?]$/.test(before));
          if (start) {
            const [first, ...more] = [...cp];
            out += first.toUpperCase() + more.join("");
          } else out += cp;
          if (dot && !/[.!?]$/.test(cp)) out += ".";
        } else if (rest === "" || isWs(rest[0])) {
          out = trimEnd(out);
        }
      }
    }
    return out + rest;
  }

  const api = { fill };
  if (typeof module === "object" && module && module.exports) module.exports = api;
  else root.PersonaFill = api;
})(typeof window !== "undefined" ? window : globalThis);
