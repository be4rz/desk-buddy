//! Order-preserving JSON edits: `set` and `remove` splice one value into or out of a JSON text,
//! leaving every other byte (key order, hand formatting, one-line entries) as it was. The studio
//! edits `pack.json` with them; serde_json would sort keys and reflow the file.
//!
//! New values are written on one line, `{"a": 1, "b": [2, 3]}`, the style of the bear's manifest,
//! with keys in the order the value's text gives them (`one_line`).

use crate::{err, Error, Result};

/// One step of a path into a JSON document.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

impl From<&str> for Seg {
    fn from(s: &str) -> Seg {
        Seg::Key(s.to_string())
    }
}
impl From<usize> for Seg {
    fn from(i: usize) -> Seg {
        Seg::Index(i)
    }
}

/// A parsed value: its byte span and, for containers, its members.
#[derive(Debug)]
struct Node {
    start: usize,
    end: usize,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    /// (decoded key, the key's start byte, the value)
    Object(Vec<(String, usize, Node)>),
    Array(Vec<Node>),
    Scalar,
}

struct P<'a> {
    b: &'a [u8],
    s: &'a str,
    i: usize,
    depth: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn fail<T>(&self, what: &str) -> Result<T> {
        let line = self.s[..self.i.min(self.s.len())].matches('\n').count() + 1;
        err(format!("JSON line {line}: {what}"))
    }
    fn value(&mut self) -> Result<Node> {
        self.ws();
        let start = self.i;
        if self.depth > 128 {
            return self.fail("nested too deep");
        }
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                self.depth += 1;
                let mut m = vec![];
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.b.get(self.i) != Some(&b'"') {
                            return self.fail("expected a key");
                        }
                        let ks = self.i;
                        self.string()?;
                        let key: String = serde_json::from_str(&self.s[ks..self.i]).map_err(|e| Error(format!("JSON key: {e}")))?;
                        if m.iter().any(|(k, _, _): &(String, usize, Node)| *k == key) {
                            return self.fail(&format!("key “{key}” appears twice"));
                        }
                        self.ws();
                        if self.b.get(self.i) != Some(&b':') {
                            return self.fail("expected ':'");
                        }
                        self.i += 1;
                        let v = self.value()?;
                        m.push((key, ks, v));
                        self.ws();
                        match self.b.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.fail("expected ',' or '}'"),
                        }
                    }
                }
                self.depth -= 1;
                Ok(Node { start, end: self.i, kind: Kind::Object(m) })
            }
            Some(b'[') => {
                self.i += 1;
                self.depth += 1;
                let mut a = vec![];
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                } else {
                    loop {
                        a.push(self.value()?);
                        self.ws();
                        match self.b.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.fail("expected ',' or ']'"),
                        }
                    }
                }
                self.depth -= 1;
                Ok(Node { start, end: self.i, kind: Kind::Array(a) })
            }
            Some(b'"') => {
                self.string()?;
                Ok(Node { start, end: self.i, kind: Kind::Scalar })
            }
            Some(_) => {
                while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r' | b':') {
                    self.i += 1;
                }
                let tok = &self.s[start..self.i];
                if tok.is_empty() || serde_json::from_str::<serde_json::Value>(tok).is_err() {
                    return self.fail(&format!("bad value “{}”", tok.chars().take(20).collect::<String>()));
                }
                Ok(Node { start, end: self.i, kind: Kind::Scalar })
            }
            None => self.fail("unexpected end"),
        }
    }
    fn string(&mut self) -> Result<()> {
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok(());
                }
                c if c < 0x20 => return self.fail("control character in a string"),
                _ => self.i += 1,
            }
        }
        self.fail("unterminated string")
    }
}

fn parse(text: &str) -> Result<Node> {
    let mut p = P { b: text.as_bytes(), s: text, i: 0, depth: 0 };
    let n = p.value()?;
    p.ws();
    if p.i != text.len() {
        return p.fail("text after the value");
    }
    // the span parser is lenient on escapes and numbers; serde_json has the last word
    serde_json::from_str::<serde_json::Value>(text).map_err(|e| Error(format!("JSON: {e}")))?;
    Ok(n)
}

/// Check that `text` is one JSON value.
pub fn validate(text: &str) -> Result<()> {
    parse(text).map(|_| ())
}

/// `text` (one JSON value) on one line: `": "` after keys, `", "` between members, keys in the
/// text's order.
pub fn one_line(text: &str) -> Result<String> {
    let n = parse(text)?;
    let mut out = String::new();
    emit(text, &n, &mut out);
    Ok(out)
}

fn emit(text: &str, n: &Node, out: &mut String) {
    match &n.kind {
        Kind::Scalar => out.push_str(&text[n.start..n.end]),
        Kind::Object(m) => {
            out.push('{');
            for (i, (_, ks, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let mut p = P { b: text.as_bytes(), s: text, i: *ks, depth: 0 };
                let _ = p.string();
                out.push_str(&text[*ks..p.i]);
                out.push_str(": ");
                emit(text, v, out);
            }
            out.push('}');
        }
        Kind::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit(text, v, out);
            }
            out.push(']');
        }
    }
}

fn show(path: &[Seg]) -> String {
    let mut s = String::new();
    for p in path {
        match p {
            Seg::Key(k) => {
                if !s.is_empty() {
                    s.push('.');
                }
                s.push_str(k)
            }
            Seg::Index(i) => s.push_str(&format!("[{i}]")),
        }
    }
    if s.is_empty() {
        "the document".into()
    } else {
        s
    }
}

fn find<'n>(root: &'n Node, path: &[Seg]) -> Result<&'n Node> {
    let mut n = root;
    for (k, seg) in path.iter().enumerate() {
        n = match (&n.kind, seg) {
            (Kind::Object(m), Seg::Key(key)) => m.iter().find(|(kk, _, _)| kk == key).map(|(_, _, v)| v),
            (Kind::Array(a), Seg::Index(i)) => a.get(*i),
            _ => None,
        }
        .ok_or_else(|| Error(format!("{}: not found", show(&path[..=k]))))?;
    }
    Ok(n)
}

/// The text between the opening bracket (or the member before) and a member's start: the
/// indentation to copy for a new member.
fn lead(text: &str, prev_end: usize, start: usize) -> String {
    let gap = &text[prev_end..start];
    match gap.rfind('\n') {
        Some(i) => gap[i..].to_string(),
        None => " ".into(),
    }
}

/// Set the value at `path` to `value_json` (any JSON text; written on one line): replaces the
/// value there, or adds the last key to its object (at the end), or appends to an array when the
/// last index is its length. Every container on the way must exist.
pub fn set(text: &str, path: &[Seg], value_json: &str) -> Result<String> {
    let v = one_line(value_json)?;
    let root = parse(text)?;
    let Some((last, parent_path)) = path.split_last() else {
        return Ok(v);
    };
    let parent = find(&root, parent_path)?;
    let at = |a: usize, b: usize, s: &str| format!("{}{}{}", &text[..a], s, &text[b..]);
    match (&parent.kind, last) {
        (Kind::Object(m), Seg::Key(key)) => {
            if let Some((_, _, n)) = m.iter().find(|(k, _, _)| k == key) {
                return Ok(at(n.start, n.end, &v));
            }
            let member = format!("{}: {v}", serde_json::to_string(key).expect("json"));
            match m.last() {
                Some((_, ks, n)) => {
                    let prev_end = if m.len() > 1 { m[m.len() - 2].2.end } else { parent.start + 1 };
                    let ws = lead(text, prev_end, *ks);
                    Ok(at(n.end, n.end, &format!(",{ws}{member}")))
                }
                None => Ok(at(parent.start, parent.end, &format!("{{{member}}}"))),
            }
        }
        (Kind::Array(a), Seg::Index(i)) => {
            if let Some(n) = a.get(*i) {
                return Ok(at(n.start, n.end, &v));
            }
            if *i != a.len() {
                return err(format!("{}: past the end (length {})", show(path), a.len()));
            }
            match a.last() {
                Some(n) => {
                    let prev_end = if a.len() > 1 { a[a.len() - 2].end } else { parent.start + 1 };
                    let ws = lead(text, prev_end, n.start);
                    Ok(at(n.end, n.end, &format!(",{ws}{v}")))
                }
                None => Ok(at(parent.start, parent.end, &format!("[{v}]"))),
            }
        }
        (Kind::Object(_), Seg::Index(_)) => err(format!("{}: an object, not a list", show(parent_path))),
        (Kind::Array(_), Seg::Key(_)) => err(format!("{}: a list, not an object", show(parent_path))),
        (Kind::Scalar, _) => err(format!("{}: not an object or a list", show(parent_path))),
    }
}

/// Remove the member at `path` (and its comma). Removing a missing object key is not an error.
pub fn remove(text: &str, path: &[Seg]) -> Result<String> {
    let root = parse(text)?;
    let Some((last, parent_path)) = path.split_last() else {
        return err("can't remove the whole document");
    };
    let parent = find(&root, parent_path)?;
    // (start, end) of every member, start = the key for objects
    let spans: Vec<(usize, usize)> = match &parent.kind {
        Kind::Object(m) => m.iter().map(|(_, ks, n)| (*ks, n.end)).collect(),
        Kind::Array(a) => a.iter().map(|n| (n.start, n.end)).collect(),
        Kind::Scalar => return err(format!("{}: not an object or a list", show(parent_path))),
    };
    let idx = match (&parent.kind, last) {
        (Kind::Object(m), Seg::Key(key)) => match m.iter().position(|(k, _, _)| k == key) {
            Some(i) => i,
            None => return Ok(text.to_string()),
        },
        (Kind::Array(a), Seg::Index(i)) if *i < a.len() => *i,
        _ => return err(format!("{}: not found", show(path))),
    };
    let cut = |a: usize, b: usize| format!("{}{}", &text[..a], &text[b..]);
    Ok(if spans.len() == 1 {
        // the only member: leave an empty container
        cut(parent.start + 1, parent.end - 1)
    } else if idx + 1 < spans.len() {
        cut(spans[idx].0, spans[idx + 1].0)
    } else {
        cut(spans[idx - 1].1, spans[idx].1)
    })
}

/// The JSON text of the value at `path`, if there is one.
pub fn get<'t>(text: &'t str, path: &[Seg]) -> Option<&'t str> {
    let root = parse(text).ok()?;
    find(&root, path).ok().map(|n| &text[n.start..n.end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "{\n  \"id\": \"bear\",\n  \"t\": {\"a\": 1, \"b\": 2},\n  \"pool\": [\n    {\"x\": 1},\n    {\"x\": 2}\n  ],\n  \"e\": []\n}\n";

    fn k(s: &str) -> Seg {
        s.into()
    }

    #[test]
    fn set_replaces_in_place() {
        let t = set(DOC, &[k("t"), k("b")], "3").unwrap();
        assert_eq!(t, DOC.replace("\"b\": 2", "\"b\": 3"));
        let t = set(DOC, &[k("pool"), 1.into()], "{ \"y\" :[1,2] , \"x\":9 }").unwrap();
        assert!(t.contains("    {\"y\": [1, 2], \"x\": 9}\n  ],"), "{t}");
    }

    #[test]
    fn set_adds_with_the_neighbours_indent() {
        let t = set(DOC, &[k("name")], "\"Bear\"").unwrap();
        assert!(t.ends_with("  \"e\": [],\n  \"name\": \"Bear\"\n}\n"), "{t}");
        let t = set(DOC, &[k("t"), k("c")], "true").unwrap();
        assert!(t.contains("{\"a\": 1, \"b\": 2, \"c\": true}"), "{t}");
        let t = set(DOC, &[k("pool"), 2.into()], "{\"x\": 3}").unwrap();
        assert!(t.contains("    {\"x\": 2},\n    {\"x\": 3}\n  ]"), "{t}");
        let t = set(DOC, &[k("e"), 0.into()], "\"q\"").unwrap();
        assert!(t.contains("\"e\": [\"q\"]"), "{t}");
        assert!(set(DOC, &[k("pool"), 5.into()], "1").is_err());
        assert!(set(DOC, &[k("nope"), k("x")], "1").is_err());
        assert!(set(DOC, &[k("t"), k("a")], "{bad").is_err());
        serde_json::from_str::<serde_json::Value>(&set(DOC, &[k("z")], "{\"a\": [1]}").unwrap()).unwrap();
    }

    #[test]
    fn remove_takes_the_comma() {
        let t = remove(DOC, &[k("t")]).unwrap();
        assert!(t.starts_with("{\n  \"id\": \"bear\",\n  \"pool\""), "{t}");
        let t = remove(DOC, &[k("e")]).unwrap();
        assert!(t.ends_with("  ]\n}\n"), "{t}");
        let t = remove(DOC, &[k("pool"), 0.into()]).unwrap();
        assert!(t.contains("[\n    {\"x\": 2}\n  ]"), "{t}");
        let t = remove(DOC, &[k("pool"), 1.into()]).unwrap();
        assert!(t.contains("[\n    {\"x\": 1}\n  ]"), "{t}");
        let t = remove(&remove(DOC, &[k("pool"), 1.into()]).unwrap(), &[k("pool"), 0.into()]).unwrap();
        assert!(t.contains("\"pool\": [],"), "{t}");
        assert_eq!(remove(DOC, &[k("missing")]).unwrap(), DOC);
        assert!(remove(DOC, &[k("pool"), 7.into()]).is_err());
        for t in [remove(DOC, &[k("id")]).unwrap(), remove(DOC, &[k("t"), k("a")]).unwrap()] {
            serde_json::from_str::<serde_json::Value>(&t).unwrap();
        }
    }

    #[test]
    fn parse_errors_name_a_line() {
        assert!(validate("{\n\"a\": 1,\n}").unwrap_err().0.contains("line"));
        assert!(validate("{\"a\": 1, \"a\": 2}").unwrap_err().0.contains("twice"));
        assert!(validate("[1] x").is_err());
        assert!(validate("\"\\u12\"").is_err());
        assert_eq!(one_line("{ \"a\" : [ ] , \"b\":{}}").unwrap(), "{\"a\": [], \"b\": {}}");
        assert_eq!(get(DOC, &[k("t"), k("a")]), Some("1"));
    }

    #[test]
    fn the_bear_round_trips() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mascots/bear/pack.json")).unwrap();
        let t = set(&text, &[k("name")], "\"Polar bear\"").unwrap();
        assert_eq!(t, text);
        let t = set(&text, &[k("description")], "\"A bear\"").unwrap();
        assert_eq!(remove(&t, &[k("description")]).unwrap(), text);
    }
}
