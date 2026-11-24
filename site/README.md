# 2poke landing page

A static site with no build step: `index.html`, `css/site.css`, four ES modules in `js/`, and the
bear's art in `assets/`. The only server code is the waitlist endpoint in `functions/api/waitlist.js`
(a Cloudflare Pages Function).

## The bear's art

The page never draws the bear. Every sprite is a clip from the Blender pipeline
(`experiments/mascot/out/bear3d/`) and every room piece comes from `rooms/starter/`.
`tools/export_assets.py` copies a curated set of clips, packs each one into a sheet (one row per
direction or set placement, one column per frame), stores it as an exact palette PNG, and writes
`assets/bear.json`, the manifest `js/sprites.js` reads.

```sh
python3 site/tools/export_assets.py   # ~70 s, deterministic; run it after any re-render
```

Re-run it whenever the pipeline output changes, then reload the page.

## Run it locally

```sh
python3 -m http.server 8780 --directory site
# open http://127.0.0.1:8780/
```

A plain static server has no `/api/waitlist`, so the form reports a network error there. Set the
`waitlist-endpoint` meta in `index.html` to an empty string to get the preview message instead,
or run the real function with Wrangler:

```sh
npx wrangler pages dev site --kv WAITLIST
```

## Files

| Path | What it is |
| --- | --- |
| `index.html` | The page: hero desk, activity strip, a day, talk, senses, rooms, privacy, studio, FAQ, join |
| `css/site.css` | All styles. Tokens come from the app's bubble (`crates/buddy-app/web/bubble/bubble.css`) |
| `js/sprites.js` | Manifest loader, frame clock, `Actor` (clips, turns, blinks, talking mouth), room drawing |
| `js/hero.js` | The bear on the mock desktop: poke, drag onto windows, cursor senses, idle life |
| `js/sections.js` | The day timeline, the talk demo, senses cards, playground, studio turntable, strip, forms |
| `js/main.js` | Boot, clock, nav, the mock tray menu, the music window |
| `functions/api/waitlist.js` | `POST /api/waitlist` into the `WAITLIST` KV namespace |
| `_headers` | Security headers and cache rules (Cloudflare Pages and Netlify read this format) |
| `404.html`, `robots.txt`, `sitemap.xml` | The usual |
| `tools/export_assets.py` | Pipeline art to `assets/` |

## Deploy (Cloudflare Pages)

1. Create a KV namespace: `npx wrangler kv namespace create WAITLIST`.
2. Create a Pages project with `site/` as the output directory and no build command
   (or `npx wrangler pages deploy site --project-name 2poke`).
3. In the project's settings, bind the namespace to the variable name `WAITLIST`
   (Settings ▸ Functions ▸ KV namespace bindings).
4. Point the domain at it. The page uses `https://2poke.app/` in its canonical, Open Graph and
   sitemap URLs; change those four places (`index.html`, `robots.txt`, `sitemap.xml`) if the domain differs.

Without the binding the endpoint answers 503 and the form shows "The waitlist is not set up yet."

### The waitlist endpoint

`POST /api/waitlist` with JSON `{"email", "company", "source"}`:

| Case | Status | Body |
| --- | --- | --- |
| New address | 201 | `{"ok": true, "already": false}` |
| Already on the list | 200 | `{"ok": true, "already": true}` |
| Honeypot `company` filled | 200 | `{"ok": true, "already": false}` (nothing stored) |
| Bad email | 422 | `{"ok": false, "error": "…"}` |
| Not JSON | 400 / 415 | `{"ok": false, "error": "…"}` |
| No KV binding | 503 | `{"ok": false, "error": "…"}` |
| Other methods | 405 | `{"ok": false, "error": "Use POST."}` |

Each address is stored once under `email:<lowercased address>` as
`{"email", "source", "at", "country"}`. Export the list with
`npx wrangler kv key list --binding WAITLIST` (or the dashboard).

Another host works too: serve `site/` as static files and answer `POST /api/waitlist` with the
same JSON contract.

## Social image

`assets/og.png` (1200×630) is rendered from `tools/og.html`, a page built from the same sprites.
Open it at 1200×630 and capture it with a headless browser to regenerate it.

## Fonts

All bundled in `assets/fonts/`, under the SIL Open Font License 1.1, whose text and copyright lines
are in `assets/fonts/OFL.txt`: Nunito (Vernon Adams, Cyreal, Jacques Le Bailly), Figtree (Erik
Kennedy) and Silkscreen (Jason Kottke). Keep `OFL.txt` beside the fonts wherever they are deployed.

The rest of `assets/` is the bear's art, under CC BY-NC 4.0 (`ARTWORK.md` at the repository root).

## Accessibility and motion

- Every demo is also readable as text: speech goes to a live region, controls are real buttons.
- `prefers-reduced-motion` stops the strip marquee, wandering and auto-advance; the bear still
  answers pokes.
- Animation runs only while a section is on screen, and stops in background tabs.
