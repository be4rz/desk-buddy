// Unit checks of harness-depth's pure parts: capping, clearing, compaction
// sizing, the web tools' parsing and address checks, the helpers' env.

import { expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { CLEARED, capResult, clearOld, compactionFor, contextSettings, estimateTokens, head, outputName } from "../../src/ext/context/index.ts";
import { imageType } from "../../src/ext/look/index.ts";
import { commandEnv } from "../../src/ext/tools/index.ts";
import { checkUrl, decodeEntities, htmlToText, isPrivateIp, parseDdg } from "../../src/ext/web/index.ts";
import type { Init } from "../../src/protocol.ts";
import { DDG_HTML } from "../support/ddg.ts";

const S = { maxLines: 200, maxBytes: 16384, clearBatch: 8, compactPct: 80 };

test("compaction: reserve is 20% of the window, the rest scaled and capped", () => {
  expect(compactionFor(1000, 80)).toEqual({ reserveTokens: 200, keepRecentTokens: 250, backgroundTokens: 100 });
  expect(compactionFor(200000, 80)).toEqual({ reserveTokens: 40000, keepRecentTokens: 20000, backgroundTokens: 20000 });
  expect(compactionFor(1000001, 80).reserveTokens).toBe(200001); // ceil
  expect(compactionFor(1_000_000, 80).backgroundTokens).toBe(32768);
});

test("context settings: defaults, and bad values fall back", () => {
  const init = (context: unknown) => ({ settings: { context } }) as unknown as Init;
  expect(contextSettings(init(undefined))).toEqual(S);
  expect(contextSettings(init({ maxLines: 10, clearBatch: -1, maxBytes: "x" }))).toEqual({ ...S, maxLines: 10 });
});

test("cap: 500 lines keep the first 200 and a footer; the full text is written", () => {
  const dir = mkdtempSync(join(tmpdir(), "cap-"));
  const full = Array.from({ length: 500 }, (_, i) => `line ${i + 1}`).join("\n");
  const out = capResult({ output: [{ type: "text", text: full }] }, "call_7/../x", dir, S)!;
  const text = (out.output![0] as { text: string }).text;
  expect(text.split("\n").slice(0, 200).at(-1)).toBe("line 200");
  expect(text).not.toContain("line 201");
  expect(text.endsWith("[output capped: full text in .buddy/outputs/call_7_.._x.txt]")).toBe(true);
  expect(readFileSync(join(dir, ".buddy", "outputs", "call_7_.._x.txt"), "utf8")).toBe(full);
});

test("cap: bytes too, never mid-character; small results untouched", () => {
  const dir = mkdtempSync(join(tmpdir(), "cap-"));
  const big = "é".repeat(20000); // 40 000 bytes, one line
  const out = capResult({ output: [{ type: "text", text: big }], isError: true }, "c1", dir, S)!;
  const text = (out.output![0] as { text: string }).text;
  expect(Buffer.byteLength(text)).toBeLessThanOrEqual(S.maxBytes);
  expect(text).not.toContain("�");
  expect(out.isError).toBe(true);
  expect(capResult({ output: [{ type: "text", text: "fine" }] }, "c2", dir, S)).toBeUndefined();
  // Exactly maxLines lines with a final newline fit.
  expect(capResult({ output: [{ type: "text", text: "l\n".repeat(200) }] }, "c4", dir, S)).toBeUndefined();
  expect(capResult({ output: [{ type: "image", data: "AAAA", mimeType: "image/png" }] }, "c3", dir, S)).toBeUndefined();
});

test("cap: a helper's result is cut with a footer and no file, since it can't read one", () => {
  const dir = mkdtempSync(join(tmpdir(), "buddy-cap-helper-"));
  const out = capResult({ output: [{ type: "text", text: "x\n".repeat(500) }] }, "h1", dir, S, false)!;
  const text = (out.output![0] as { text: string }).text;
  expect(text.endsWith("[output capped: the rest of this result isn't shown]")).toBe(true);
  expect(text.split("\n").filter((l) => l === "x")).toHaveLength(200);
  expect(existsSync(join(dir, ".buddy"))).toBe(false);
});

test("output names are file names, never paths", () => {
  expect(outputName("../../etc/passwd")).toBe("__.._etc_passwd.txt");
  expect(outputName("")).toBe("call.txt");
  expect(head("a\nb\nc", 2, 100)).toBe("a\nb");
});

const result = (n: number, size: number) => ({ role: "toolResult", toolCallId: `c${n}`, content: [{ type: "text", text: "x".repeat(size) }] });

test("clearing: nothing under 60% of the window", () => {
  const msgs = [{ role: "user", content: "hi" }, ...Array.from({ length: 10 }, (_, i) => result(i, 400))];
  expect(clearOld(msgs, 10000, 2)).toBeUndefined();
});

test("clearing: oldest first, whole batches, the newest batch kept", () => {
  // 20 results of 1 000 tokens: 20 000 tokens against a 20 000 window (60% = 12 000).
  const msgs = [{ role: "user", content: "hi" }, ...Array.from({ length: 20 }, (_, i) => result(i, 4000))];
  const out = clearOld(msgs, 20000, 4)!;
  const cleared = out.map((m, i) => (Array.isArray(m.content) && (m.content[0] as { text: string }).text === CLEARED ? i : -1)).filter((i) => i >= 0);
  expect(cleared.length % 4).toBe(0);
  expect(cleared[0]).toBe(1);
  expect(cleared).toEqual(Array.from({ length: cleared.length }, (_, i) => i + 1));
  expect(estimateTokens(out)).toBeLessThanOrEqual(12000);
  // The input is untouched.
  expect((msgs[1]!.content as { text: string }[])[0]!.text).toHaveLength(4000);
  // Fewer results than two batches: the newest batch is never cleared, so nothing to do.
  expect(clearOld(msgs.slice(0, 5), 1000, 4)).toBeUndefined();
});

test("web: private and loopback addresses", () => {
  for (const ip of ["127.0.0.1", "10.1.2.3", "192.168.0.1", "172.16.5.5", "169.254.1.1", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"]) expect(isPrivateIp(ip)).toBe(true);
  // Any spelling of an embedded IPv4 address: mapped hex (what URL parsing yields), compatible, NAT64, 6to4.
  for (const ip of ["::ffff:7f00:1", "::ffff:a00:1", "::127.0.0.1", "64:ff9b::7f00:1", "2002:c0a8:101::", "fe80::1%en0", "ff02::1", "fec0::1", "::ffff:0:10.0.0.1"]) expect(isPrivateIp(ip)).toBe(true);
  for (const ip of ["93.184.216.34", "1.1.1.1", "172.32.0.1", "2606:4700::1111", "::ffff:808:808"]) expect(isPrivateIp(ip)).toBe(false);
});

test("web: refused URLs", async () => {
  await expect(checkUrl("file:///etc/passwd", false)).rejects.toThrow("Only http and https");
  await expect(checkUrl("http://localhost:8080/", false)).rejects.toThrow("Local addresses");
  await expect(checkUrl("http://127.0.0.1/", false)).rejects.toThrow("private network");
  await expect(checkUrl("http://[::1]/", false)).rejects.toThrow("private network");
  await expect(checkUrl("http://[::ffff:127.0.0.1]:11434/", false)).rejects.toThrow("private network");
  await expect(checkUrl("http://user:pw@example.com/", false)).rejects.toThrow("user name");
  await expect(checkUrl("not a url", false)).rejects.toThrow("Not a URL");
  expect((await checkUrl("http://127.0.0.1:9/", true)).port).toBe("9");
});



test("web: DuckDuckGo results, redirects unwrapped and ads skipped", () => {
  expect(parseDdg(DDG_HTML)).toEqual([
    { title: "Polar & bears", url: "https://example.org/bears?a=1&b=2", snippet: "Polar bears live in the Arctic." },
    { title: "Second", url: "https://second.example/page", snippet: "Two — snippet" },
  ]);
  expect(parseDdg("<html>no results</html>")).toEqual([]);
});

test("web: HTML to text", () => {
  const t = htmlToText("<html><head><title>T &amp; U</title><style>p{}</style></head><body><script>evil()</script><p>One&nbsp;two</p><div>Three</div></body></html>");
  expect(t).toBe("# T & U\n\nOne two\n\nThree");
  expect(htmlToText("<p>a</p><script>never closed <p>b")).toBe("a");
  // `</head>` is optional; offsets survive letters whose lower case is longer (İ); close tags match exactly.
  expect(htmlToText("<html><head><title>T</title><body><p>Hello body</p></body></html>")).toBe("# T\n\nHello body");
  expect(htmlToText("<head><title>Başlık İ</title></head><p>İİİİ</p><script>x()</script><p>one</p><!-- c --><p>two</p>")).toBe("# Başlık İ\n\nİİİİ\n\none\n\ntwo");
  expect(htmlToText("<script>var a='</scripty>';</script><p>ok</p>")).toBe("ok");
  expect(htmlToText("<p>a</p><script>x</script foo><p>b</p><style>y</style/><p>c</p>")).toBe("a\n\nb\n\nc");
  expect(htmlToText("<script-x>a</script-x><p>b</p><title-bar>t</title-bar>")).toBe("a\nb\nt");
  // Hostile input stays linear: 2 MB of unclosed tags, well under a second.
  const t0 = performance.now();
  for (const bad of ["<script>".repeat(250_000), "<".repeat(2_000_000), "<!--".repeat(500_000)]) htmlToText(bad);
  expect(performance.now() - t0).toBeLessThan(1000);
  expect(decodeEntities("&#x1F43B; &lt;&gt; &bogus;")).toBe("🐻 <> &bogus;");
});

test("look: image types by their bytes", () => {
  expect(imageType(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d]))).toBe("image/png");
  expect(imageType(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]))).toBe("image/jpeg");
  expect(imageType(new TextEncoder().encode("GIF89a"))).toBe("image/gif");
  expect(imageType(new TextEncoder().encode("RIFF1234WEBPVP8"))).toBe("image/webp");
  expect(imageType(new TextEncoder().encode("hello"))).toBeUndefined();
});

test("a helper conversation's command gets no BUDDY_* variables", () => {
  const init = { shellEnv: { PATH: "/bin" }, host: { url: "http://127.0.0.1:1", token: "tok-secret" }, platform: "macos" } as unknown as Init;
  expect(commandEnv(init, "u:1", true)).toMatchObject({ BUDDY_API: "http://127.0.0.1:1", BUDDY_TOKEN: "tok-secret", BUDDY_TURN: "u:1" });
  const child = commandEnv(init, "u:1", false);
  expect(Object.keys(child).filter((k) => k.startsWith("BUDDY_"))).toEqual([]);
});
