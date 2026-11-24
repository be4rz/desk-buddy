// POST /api/waitlist: a Cloudflare Pages Function that keeps waitlist sign-ups
// in a KV namespace bound as WAITLIST (see site/README.md).
//
// Request:  {"email": "you@example.com", "company": "", "source": "hero" | "final"}
// Response: {"ok": true, "already": false} | {"ok": false, "error": "..."}
//
// `company` is a honeypot field that people never see; a filled one is
// answered like a success and stored nowhere.

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
const SOURCES = new Set(['hero', 'final']);

function json(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
  });
}

export async function onRequestPost({ request, env }) {
  const type = request.headers.get('content-type') || '';
  if (!type.includes('application/json')) return json({ ok: false, error: 'Send JSON.' }, 415);
  if (Number(request.headers.get('content-length') || 0) > 2048) return json({ ok: false, error: 'Too large.' }, 413);

  let body;
  try {
    body = await request.json();
  } catch {
    return json({ ok: false, error: 'That was not valid JSON.' }, 400);
  }
  if (!body || typeof body !== 'object') return json({ ok: false, error: 'That was not valid JSON.' }, 400);

  if (typeof body.company === 'string' && body.company.trim() !== '') return json({ ok: true, already: false });

  const email = typeof body.email === 'string' ? body.email.trim().toLowerCase() : '';
  if (email.length > 254 || !EMAIL.test(email)) return json({ ok: false, error: 'That email address doesn’t look right.' }, 422);

  if (!env.WAITLIST) return json({ ok: false, error: 'The waitlist is not set up yet.' }, 503);

  const key = `email:${email}`;
  if (await env.WAITLIST.get(key)) return json({ ok: true, already: true });

  const record = {
    email,
    source: SOURCES.has(body.source) ? body.source : 'other',
    at: new Date().toISOString(),
    country: request.cf?.country || null,
  };
  await env.WAITLIST.put(key, JSON.stringify(record));
  return json({ ok: true, already: false }, 201);
}

export function onRequest({ request }) {
  if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: { allow: 'POST, OPTIONS' } });
  return json({ ok: false, error: 'Use POST.' }, 405);
}
