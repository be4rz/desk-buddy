// Evaluated by the app in the browser widget's content webview (no IPC there) through
// WKWebView evaluateJavaScript. `__BUDDY_OP__` is replaced with a JSON op before evaluation:
//   {op:"snapshot"} | {op:"click", ref:"e7"} | {op:"type", ref:"e3", text:"hi\n"}
// It returns a JSON string: {ok:true, url, title, text} or {ok:false, code, message}.
// Refs are written as data-buddy-ref on the elements, so they last until the next snapshot.
(function (op) {
  "use strict";
  var MAX = 40000;
  function out(v) { return JSON.stringify(v); }
  function visible(el) {
    if (!el.getClientRects().length) return false;
    var s = getComputedStyle(el);
    return s.visibility !== "hidden" && s.display !== "none" && s.opacity !== "0";
  }
  function interactive(el) {
    var t = el.tagName;
    if (t === "A" && el.hasAttribute("href")) return true;
    if (t === "BUTTON" || t === "SELECT" || t === "TEXTAREA" || t === "SUMMARY") return true;
    if (t === "INPUT") return (el.type || "").toLowerCase() !== "hidden";
    var r = el.getAttribute("role");
    if (r && /^(button|link|checkbox|radio|tab|menuitem|switch|textbox|searchbox|combobox|option)$/.test(r)) return true;
    return el.isContentEditable && (!el.parentElement || !el.parentElement.isContentEditable);
  }
  // A field's label, never its value: what the person typed (a password during Take over, a
  // message) stays out of the snapshot. Only a button-like input's value is its caption.
  function label(el) {
    var t = el.tagName;
    var s = el.getAttribute("aria-label") || "";
    var field = t === "INPUT" || t === "TEXTAREA";
    if (!s && field) s = el.placeholder || el.name || "";
    if (!s && !field) s = (el.innerText || el.textContent || "").replace(/\s+/g, " ").trim();
    if (!s && t === "INPUT" && /^(submit|button|reset)$/i.test(el.type || "")) s = el.value || "";
    return s.slice(0, 80);
  }
  function kind(el) {
    var t = el.tagName.toLowerCase();
    if (t === "a") return "link";
    if (t === "input") return "field:" + (el.type || "text").toLowerCase();
    if (t === "textarea") return "field";
    return el.getAttribute("role") || t;
  }
  function snapshot() {
    var old = document.querySelectorAll("[data-buddy-ref]");
    for (var i = 0; i < old.length; i++) old[i].removeAttribute("data-buddy-ref");
    var n = 0, lines = [], len = 0;
    var walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    var skip = null;
    while (walker.nextNode()) {
      var node = walker.currentNode;
      if (skip && skip.contains(node)) continue;
      skip = null;
      var line = "";
      if (node.nodeType === 1) {
        var tag = node.tagName;
        if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT" || tag === "TEMPLATE" || !visible(node)) { skip = node; continue; }
        if (interactive(node)) {
          n++;
          node.setAttribute("data-buddy-ref", "e" + n);
          line = "[e" + n + "] " + kind(node) + " “" + label(node) + "”";
          skip = node;
        } else if (/^H[1-6]$/.test(tag) && !node.querySelector("a[href],button,input,select,textarea")) {
          line = "#".repeat(+tag[1]) + " " + (node.innerText || "").replace(/\s+/g, " ").trim();
          skip = node;
        }
      } else {
        var p = node.parentElement;
        if (!p || !visible(p)) continue;
        line = node.nodeValue.replace(/\s+/g, " ").trim();
      }
      if (!line) continue;
      lines.push(line);
      len += line.length + 1;
      if (len > MAX) { lines.push("… (page text cut at " + MAX + " characters)"); break; }
    }
    return { ok: true, url: location.href, title: document.title, text: lines.join("\n"), refs: n };
  }
  function find(ref) {
    if (!/^e\d{1,5}$/.test(String(ref || ""))) return null;
    return document.querySelector('[data-buddy-ref="' + ref + '"]');
  }
  function missing(ref) { return { ok: false, code: "no_element", message: "no such element: " + ref + " (take a new snapshot)" }; }
  try {
    if (op.op === "snapshot") return out(snapshot());
    if (op.op === "click") {
      var el = find(op.ref);
      if (!el) return out(missing(op.ref));
      el.scrollIntoView({ block: "center" });
      el.click();
      return out({ ok: true, url: location.href, clicked: op.ref });
    }
    if (op.op === "type") {
      var f = find(op.ref);
      if (!f) return out(missing(op.ref));
      var text = String(op.text || "");
      var enter = /\n$/.test(text);
      if (enter) text = text.replace(/\n$/, "");
      f.focus();
      if (f.isContentEditable) { f.textContent = text; }
      else {
        var proto = f.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        var setter = Object.getOwnPropertyDescriptor(proto, "value");
        if (setter && setter.set) setter.set.call(f, text); else f.value = text;
      }
      f.dispatchEvent(new Event("input", { bubbles: true }));
      f.dispatchEvent(new Event("change", { bubbles: true }));
      if (enter) {
        var o = { key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true };
        f.dispatchEvent(new KeyboardEvent("keydown", o));
        f.dispatchEvent(new KeyboardEvent("keyup", o));
        if (f.form) { if (f.form.requestSubmit) f.form.requestSubmit(); else f.form.submit(); }
      }
      return out({ ok: true, url: location.href, typed: op.ref });
    }
    return out({ ok: false, code: "usage", message: "unknown browser op" });
  } catch (e) {
    return out({ ok: false, code: "page_error", message: String(e && e.message || e) });
  }
})(__BUDDY_OP__);
