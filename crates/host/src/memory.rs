//! `buddy memory …` (design I4): the routes, what they hand the app as a `MemoryOp`, and the
//! CLI's small helpers (request building, the one-line fact text). The store itself lives in
//! `crates/memory`; this module only checks shapes, so a sink that has no store answers 503.

use super::*;

/// The longest fact `text`, in characters.
pub const MAX_TEXT: usize = 200;
/// `search --limit` bounds and default.
pub const MAX_LIMIT: u32 = 20;
pub const DEFAULT_LIMIT: u32 = 5;
/// The strongest `importance` a fact takes (1 = routine).
pub const MAX_IMPORTANCE: u8 = 10;

/// The refusal text when no store runs.
pub const NO_MEMORY: &str = "desk-buddy's memory isn't available.";
/// The refusal text for a write that looks like a password or key.
pub const SECRET: &str = "That looks like a password or key, so I won't keep it.";
/// The refusal text while memory is switched off.
pub const MEMORY_OFF: &str = "Memory is off.";

impl Refusal {
    /// 503 `no_memory`: a sink with no store (the s2-harness, a test sink, or the app when the store failed to open).
    pub fn no_memory() -> Refusal {
        Refusal::new(503, "no_memory", NO_MEMORY)
    }

    /// 409 `memory_off`: the person turned memory off.
    pub fn memory_off() -> Refusal {
        Refusal::new(409, "memory_off", MEMORY_OFF)
    }

    /// 400 `secret`: the text looks like a password or key.
    pub fn secret() -> Refusal {
        Refusal::new(400, "secret", SECRET)
    }
}

/// What a memory command asks the app's store to do. `Recall` (p2) joins this enum.
#[derive(Clone, Debug, PartialEq)]
pub enum MemoryOp {
    /// Reply `{id, outcome: "created"|"extended"|"superseded"|"disputed"|"skipped"}`.
    Remember { text: String, key: Option<String>, qualifier: Option<String>, value: Option<String>, kind: Option<String>, importance: Option<u8>, from: Option<String> },
    /// By id or by key[:qualifier]. Reply `{forgotten: [ids]}`; 404 `not_found`.
    Forget { id: Option<i64>, key: Option<String>, qualifier: Option<String> },
    /// Reply `{facts: [Fact]}`.
    List { kind: Option<String> },
    /// Reply `{facts: [Fact]}`, best first.
    Search { q: String, limit: u32 },
    /// Reply `{}` (the app opens or focuses the window).
    Open,
}

/// The `/v1/memory/*` paths.
const ROUTES: [&str; 5] = ["/v1/memory/remember", "/v1/memory/forget", "/v1/memory/list", "/v1/memory/search", "/v1/memory/open"];

/// `pred[:qualifier]` as (predicate, qualifier).
pub fn split_key(s: &str) -> Result<(String, Option<String>), String> {
    let bad = || "A key is pred or pred:qualifier, like home_city or people:sister.".to_string();
    let s = s.trim();
    match s.split_once(':') {
        None if !s.is_empty() => Ok((s.to_string(), None)),
        Some((p, q)) if !p.trim().is_empty() && !q.trim().is_empty() => Ok((p.trim().to_string(), Some(q.trim().to_string()))),
        _ => Err(bad()),
    }
}

/// An optional string field: absent, null or blank is `None`; another type is a usage error.
fn opt_str(body: &Value, name: &str) -> Result<Option<String>, String> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        Some(_) => Err(format!("{name} must be a string.")),
    }
}

/// The `key` field as (predicate, qualifier): absent or null is `None`; a blank or odd one is a usage error.
fn opt_key(body: &Value) -> Result<Option<(String, Option<String>)>, String> {
    match body.get("key") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(k)) => split_key(k).map(Some),
        Some(_) => Err("key must be a string.".into()),
    }
}

fn remember_op(body: &Value) -> Result<MemoryOp, String> {
    let text = match body.get("text") {
        Some(Value::String(t)) => t.trim().to_string(),
        _ => String::new(),
    };
    if text.is_empty() {
        return Err("Remember what? buddy memory remember \"<text>\".".into());
    }
    if text.chars().count() > MAX_TEXT {
        return Err(format!("Keep it under {MAX_TEXT} characters."));
    }
    let (key, qualifier) = opt_key(body)?.map_or((None, None), |(p, q)| (Some(p), q));
    let importance = match body.get("importance") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) if (1..=MAX_IMPORTANCE as u64).contains(&n) => Some(n as u8),
            _ => return Err(format!("importance is a whole number from 1 to {MAX_IMPORTANCE}.")),
        },
    };
    Ok(MemoryOp::Remember { text, key, qualifier, value: opt_str(body, "value")?, kind: opt_str(body, "kind")?, importance, from: opt_str(body, "from")? })
}

fn forget_op(body: &Value) -> Result<MemoryOp, String> {
    let id = match body.get("id") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_i64() {
            Some(n) if n > 0 => Some(n),
            _ => return Err("id is the number of a fact, like 12.".into()),
        },
    };
    match (id, opt_key(body)?) {
        (Some(_), Some(_)) | (None, None) => Err("Forget which? buddy memory forget <id> or --key pred[:qualifier].".into()),
        (id, None) => Ok(MemoryOp::Forget { id, key: None, qualifier: None }),
        (None, Some((p, q))) => Ok(MemoryOp::Forget { id: None, key: Some(p), qualifier: q }),
    }
}

/// `%XX` and `+` decoded; `None` when the escape or the UTF-8 is malformed.
fn query_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// `s` percent-encoded for a query value.
pub fn query_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The query string's pairs, in order.
fn query_pairs(query: &str) -> Result<Vec<(String, String)>, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            Ok((query_decode(k).ok_or("The query isn't valid.")?, query_decode(v).ok_or("The query isn't valid.")?))
        })
        .collect()
}

fn list_op(query: &str) -> Result<MemoryOp, String> {
    let kind = query_pairs(query)?.into_iter().find(|(k, _)| k == "kind").map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    Ok(MemoryOp::List { kind })
}

fn search_op(query: &str) -> Result<MemoryOp, String> {
    let pairs = query_pairs(query)?;
    let get = |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim());
    let q = get("q").unwrap_or("");
    if q.is_empty() {
        return Err("Search for what? buddy memory search \"<words>\".".into());
    }
    let limit = match get("limit") {
        None | Some("") => DEFAULT_LIMIT,
        Some(l) => match l.parse::<u32>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => n,
            _ => return Err(format!("limit is a whole number from 1 to {MAX_LIMIT}.")),
        },
    };
    Ok(MemoryOp::Search { q: q.to_string(), limit })
}

/// Answer one `/v1/memory/*` request: check its shape, hand the `MemoryOp` to the sink, pass its reply on.
pub(crate) fn serve(sh: &Shared, req: Request, caller: Caller, turn: Option<String>, after_turn: bool, method: &Method, path: &str, body: &Value) {
    let query = req.url().split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
    let op = match (method, path) {
        (Method::Post, "/v1/memory/remember") => remember_op(body),
        (Method::Post, "/v1/memory/forget") => forget_op(body),
        (Method::Get, "/v1/memory/list") => list_op(&query),
        (Method::Get, "/v1/memory/search") => search_op(&query),
        (Method::Post, "/v1/memory/open") => Ok(MemoryOp::Open),
        (_, p) if ROUTES.contains(&p) => return refuse(req, 405, "method", "Wrong method."),
        _ => return refuse(req, 404, "not_found", "No such endpoint."),
    };
    match op {
        Err(e) => refuse(req, 400, "usage", &e),
        Ok(op) => act(sh, req, HostEvent { caller, action: Action::Memory(op), turn, after_turn }, Value::Null),
    }
}

// ------------------------------------------------------------------ the CLI's helpers

/// `2026-10` (or `2026-10-01`) as `Oct 2026`; anything else as it came.
pub fn since_label(since: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut it = since.split('-');
    match (it.next(), it.next().and_then(|m| m.parse::<usize>().ok())) {
        (Some(y), Some(m)) if y.len() == 4 && (1..=12).contains(&m) => format!("{} {y}", MONTHS[m - 1]),
        _ => since.to_string(),
    }
}

/// One line per fact: `#12 Lives in Hanoi (since Oct 2026)`.
pub fn fact_line(fact: &Value) -> String {
    let id = fact["id"].as_i64().map_or_else(|| "?".to_string(), |n| n.to_string());
    let text = fact["text"].as_str().unwrap_or("");
    match fact["since"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => format!("#{id} {text} (since {})", since_label(s)),
        None => format!("#{id} {text}"),
    }
}

/// A CLI flag's value: `--name v` or `--name=v`.
fn flag<'a>(w: &'a str, name: &str, it: &mut std::slice::Iter<'a, &'a str>) -> Option<Result<String, String>> {
    if w == name {
        Some(it.next().map(|v| v.to_string()).ok_or_else(|| format!("{name} needs a value")))
    } else {
        w.strip_prefix(name).and_then(|r| r.strip_prefix('=')).map(|v| Ok(v.to_string()))
    }
}

/// `buddy memory remember …`'s arguments as the route's body.
pub fn cli_remember(args: &[&str]) -> Result<Value, String> {
    let mut text = Vec::new();
    let mut body = json!({});
    let mut it = args.iter();
    while let Some(w) = it.next() {
        let mut taken = false;
        for (name, field) in [("--key", "key"), ("--value", "value"), ("--kind", "kind"), ("--from", "from")] {
            if let Some(v) = flag(w, name, &mut it) {
                body[field] = json!(v?);
                taken = true;
                break;
            }
        }
        if taken {
            continue;
        }
        if let Some(v) = flag(w, "--importance", &mut it) {
            let v = v?;
            let n: u8 = v.parse().ok().filter(|n| (1..=MAX_IMPORTANCE).contains(n)).ok_or_else(|| format!("--importance is a whole number from 1 to {MAX_IMPORTANCE}"))?;
            body["importance"] = json!(n);
        } else if w.starts_with("--") {
            return Err(format!("unexpected “{w}”: buddy memory remember \"<text>\" [--key pred[:qualifier]] [--value V] [--kind K] [--importance N] [--from DATE]"));
        } else {
            text.push(*w);
        }
    }
    let text = text.join(" ");
    if text.trim().is_empty() {
        return Err("remember what? buddy memory remember \"<text>\"".into());
    }
    body["text"] = json!(text);
    Ok(body)
}

/// `buddy memory forget <id> | --key pred[:qualifier]`'s arguments as the route's body.
pub fn cli_forget(args: &[&str]) -> Result<Value, String> {
    const USAGE: &str = "buddy memory forget <id> | --key pred[:qualifier]";
    let mut it = args.iter();
    let (mut id, mut key) = (None, None);
    while let Some(w) = it.next() {
        if let Some(v) = flag(w, "--key", &mut it) {
            key = Some(v?);
        } else if id.is_none() && !w.starts_with('-') {
            id = Some(w.trim_start_matches('#').parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(|| format!("“{w}” isn’t a fact number: {USAGE}"))?);
        } else {
            return Err(format!("unexpected “{w}”: {USAGE}"));
        }
    }
    match (id, key) {
        (Some(id), None) => Ok(json!({ "id": id })),
        (None, Some(key)) => Ok(json!({ "key": key })),
        _ => Err(USAGE.into()),
    }
}

/// `buddy memory list [--kind K]`'s route path.
pub fn cli_list(args: &[&str]) -> Result<String, String> {
    match args {
        [] => Ok("/v1/memory/list".into()),
        ["--kind", k] => Ok(format!("/v1/memory/list?kind={}", query_encode(k))),
        [k] if k.starts_with("--kind=") => Ok(format!("/v1/memory/list?kind={}", query_encode(&k["--kind=".len()..]))),
        _ => Err("buddy memory list [--kind K]".into()),
    }
}

/// `buddy memory search "<words>" [--limit N]`'s route path.
pub fn cli_search(args: &[&str]) -> Result<String, String> {
    let (mut words, mut limit) = (Vec::new(), None);
    let mut it = args.iter();
    while let Some(w) = it.next() {
        if let Some(v) = flag(w, "--limit", &mut it) {
            let v = v?;
            limit = Some(v.parse::<u32>().ok().filter(|n| (1..=MAX_LIMIT).contains(n)).ok_or_else(|| format!("--limit is a whole number from 1 to {MAX_LIMIT}"))?);
        } else if w.starts_with("--") {
            return Err(format!("unexpected “{w}”: buddy memory search \"<words>\" [--limit N]"));
        } else {
            words.push(*w);
        }
    }
    let q = words.join(" ");
    if q.trim().is_empty() {
        return Err("search for what? buddy memory search \"<words>\"".into());
    }
    Ok(match limit {
        Some(n) => format!("/v1/memory/search?q={}&limit={n}", query_encode(&q)),
        None => format!("/v1/memory/search?q={}", query_encode(&q)),
    })
}
