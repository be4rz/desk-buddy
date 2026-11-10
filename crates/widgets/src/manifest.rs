//! `widget.json` v1 (design §4.9, frozen). Parsed by hand from JSON so a bad file names the
//! field that is wrong (`widget.json: actions[1].name: missing`), not a serde position.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const WIDGET_VERSION: u64 = 1;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Manifest {
    pub widget_version: u64,
    pub id: String,
    pub name: String,
    /// the `buddy <cli> <action>` word; defaults to the id
    pub cli: String,
    pub entry: String,
    pub window: WindowSpec,
    pub actions: Vec<ActionSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowSpec {
    pub w: u32,
    pub h: u32,
    pub hideable: bool,
    /// hide = park the window 1×1 off-screen instead of ordering it out (the media spike's fallback)
    pub park_offscreen: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionSpec {
    pub name: String,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default)]
    pub about: String,
    #[serde(rename = "readOnly", default)]
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// takes every remaining word (the last arg only)
    #[serde(default)]
    pub rest: bool,
    #[serde(default)]
    pub optional: bool,
}

const ARG_TYPES: [&str; 3] = ["string", "number", "boolean"];

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

struct P<'a> {
    obj: &'a Map<String, Value>,
    at: String,
}

impl<'a> P<'a> {
    fn path(&self, k: &str) -> String {
        if self.at.is_empty() {
            k.to_string()
        } else {
            format!("{}.{k}", self.at)
        }
    }
    fn err(&self, k: &str, what: &str) -> String {
        format!("widget.json: {}: {what}", self.path(k))
    }
    fn str(&self, k: &str) -> Result<String, String> {
        match self.obj.get(k) {
            None | Some(Value::Null) => Err(self.err(k, "missing")),
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
            Some(Value::String(_)) => Err(self.err(k, "empty")),
            Some(_) => Err(self.err(k, "not a string")),
        }
    }
    fn opt_str(&self, k: &str) -> Result<Option<String>, String> {
        match self.obj.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.str(k).map(Some),
        }
    }
    fn bool(&self, k: &str, default: bool) -> Result<bool, String> {
        match self.obj.get(k) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(self.err(k, "not true or false")),
        }
    }
    fn size(&self, k: &str, lo: u64, hi: u64) -> Result<u32, String> {
        match self.obj.get(k).and_then(Value::as_u64) {
            Some(n) if (lo..=hi).contains(&n) => Ok(n as u32),
            Some(_) => Err(self.err(k, &format!("out of range {lo}–{hi}"))),
            None if self.obj.get(k).is_none() => Err(self.err(k, "missing")),
            None => Err(self.err(k, "not a whole number")),
        }
    }
    fn obj(&self, k: &str, required: bool) -> Result<Option<P<'a>>, String> {
        match self.obj.get(k) {
            None | Some(Value::Null) if !required => Ok(None),
            None | Some(Value::Null) => Err(self.err(k, "missing")),
            Some(Value::Object(m)) => Ok(Some(P { obj: m, at: self.path(k) })),
            Some(_) => Err(self.err(k, "not an object")),
        }
    }
}

impl Manifest {
    /// Parse and validate a `widget.json`'s bytes.
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("widget.json: not JSON ({e})"))?;
        let Value::Object(top) = &v else { return Err("widget.json: not a JSON object".into()) };
        let p = P { obj: top, at: String::new() };
        let version = match top.get("widget_version") {
            None => return Err(p.err("widget_version", "missing")),
            Some(x) => x.as_u64().ok_or_else(|| p.err("widget_version", "not a whole number"))?,
        };
        if version != WIDGET_VERSION {
            return Err(p.err("widget_version", &format!("{version} isn't supported (only {WIDGET_VERSION})")));
        }
        let id = p.str("id")?;
        if !is_word(&id) {
            return Err(p.err("id", "use a-z, 0-9, - or _ (up to 32, starting with a letter)"));
        }
        let name = p.str("name")?;
        let cli = p.opt_str("cli")?.unwrap_or_else(|| id.clone());
        if !is_word(&cli) {
            return Err(p.err("cli", "use a-z, 0-9, - or _ (up to 32, starting with a letter)"));
        }
        let entry = p.str("entry")?;
        if entry.starts_with('/') || entry.contains("..") || entry.contains('\\') {
            return Err(p.err("entry", "must be a path inside the widget"));
        }
        let w = p.obj("window", true)?.unwrap();
        let window = WindowSpec { w: w.size("w", 1, 2000)?, h: w.size("h", 1, 2000)?, hideable: w.bool("hideable", true)?, park_offscreen: w.bool("park_offscreen", false)? };
        let mut actions = vec![];
        match top.get("actions") {
            None | Some(Value::Null) => {}
            Some(Value::Array(list)) => {
                for (i, a) in list.iter().enumerate() {
                    let at = format!("actions[{i}]");
                    let Value::Object(m) = a else { return Err(format!("widget.json: {at}: not an object")) };
                    let ap = P { obj: m, at: at.clone() };
                    let name = ap.str("name")?;
                    if !is_word(&name) {
                        return Err(ap.err("name", "use a-z, 0-9, - or _"));
                    }
                    if actions.iter().any(|x: &ActionSpec| x.name == name) {
                        return Err(ap.err("name", &format!("“{name}” is listed twice")));
                    }
                    let mut args = vec![];
                    match m.get("args") {
                        None | Some(Value::Null) => {}
                        Some(Value::Array(al)) => {
                            for (j, g) in al.iter().enumerate() {
                                let gat = format!("{at}.args[{j}]");
                                let Value::Object(gm) = g else { return Err(format!("widget.json: {gat}: not an object")) };
                                let gp = P { obj: gm, at: gat };
                                let gname = gp.str("name")?;
                                let ty = gp.opt_str("type")?.unwrap_or_else(|| "string".into());
                                if !ARG_TYPES.contains(&ty.as_str()) {
                                    return Err(gp.err("type", &format!("one of {}", ARG_TYPES.join(", "))));
                                }
                                let rest = gp.bool("rest", false)?;
                                if rest && j + 1 != al.len() {
                                    return Err(gp.err("rest", "only the last arg may take the rest"));
                                }
                                args.push(ArgSpec { name: gname, ty, rest, optional: gp.bool("optional", false)? });
                            }
                        }
                        Some(_) => return Err(ap.err("args", "not a list")),
                    }
                    actions.push(ActionSpec { name, args, about: ap.opt_str("about")?.unwrap_or_default(), read_only: ap.bool("readOnly", false)? });
                }
            }
            Some(_) => return Err(p.err("actions", "not a list")),
        }
        Ok(Manifest { widget_version: version, id, name, cli, entry, window, actions })
    }

    pub fn action(&self, name: &str) -> Option<&ActionSpec> {
        self.actions.iter().find(|a| a.name == name)
    }
}

impl ActionSpec {
    /// `buddy <cli> <name> <args…>` for the help list.
    pub fn usage(&self, cli: &str) -> String {
        let mut u = format!("buddy {cli} {}", self.name);
        for a in &self.args {
            let n = if a.rest { format!("{}…", a.name) } else { a.name.clone() };
            u.push_str(&if a.optional { format!(" [{n}]") } else { format!(" <{n}>") });
        }
        u
    }

    /// The CLI's words as the action's args object; `Err` is a usage message.
    pub fn bind(&self, cli: &str, words: &[String]) -> Result<Value, String> {
        let mut out = Map::new();
        let mut i = 0;
        for a in &self.args {
            let taken: Vec<String> = if a.rest { words[i.min(words.len())..].to_vec() } else { words.get(i).cloned().into_iter().collect() };
            i += taken.len();
            if taken.is_empty() {
                if a.optional {
                    continue;
                }
                return Err(format!("{} needs <{}>: {}", self.name, a.name, self.usage(cli)));
            }
            let text = taken.join(" ");
            let v = match a.ty.as_str() {
                "number" => text.parse::<f64>().ok().and_then(|n| serde_json::Number::from_f64(n)).map(Value::Number).ok_or_else(|| format!("<{}> is a number", a.name))?,
                "boolean" => match text.as_str() {
                    "true" | "yes" | "on" => Value::Bool(true),
                    "false" | "no" | "off" => Value::Bool(false),
                    _ => return Err(format!("<{}> is true or false", a.name)),
                },
                _ => Value::String(text),
            };
            out.insert(a.name.clone(), v);
        }
        if i < words.len() {
            return Err(format!("too many words for {}: {}", self.name, self.usage(cli)));
        }
        Ok(Value::Object(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({ "widget_version": 1, "id": "media", "name": "Music", "cli": "music", "entry": "index.html",
            "window": { "w": 320, "h": 180, "hideable": true, "park_offscreen": false },
            "actions": [
                { "name": "play", "args": [{ "name": "query", "type": "string", "rest": true }], "about": "Play from YouTube", "readOnly": false },
                { "name": "pause", "args": [], "readOnly": false },
                { "name": "status", "args": [], "readOnly": true }
            ] })
    }

    fn parse(v: &Value) -> Result<Manifest, String> {
        Manifest::parse(v.to_string().as_bytes())
    }

    #[test]
    fn the_frozen_example_parses() {
        let m = parse(&good()).unwrap();
        assert_eq!((m.id.as_str(), m.cli.as_str(), m.window.w), ("media", "music", 320));
        assert_eq!(m.action("play").unwrap().usage("music"), "buddy music play <query…>");
        assert!(m.action("status").unwrap().read_only);
    }

    #[test]
    fn a_bad_widget_json_names_the_field() {
        let cases: Vec<(Box<dyn Fn(&mut Value)>, &str)> = vec![
            (Box::new(|v| v["actions"][1].as_object_mut().unwrap().remove("name").map(|_| ()).unwrap()), "widget.json: actions[1].name: missing"),
            (Box::new(|v| v["window"]["w"] = json!(0)), "widget.json: window.w: out of range 1–2000"),
            (Box::new(|v| v["widget_version"] = json!(2)), "widget.json: widget_version: 2 isn't supported (only 1)"),
            (Box::new(|v| v["id"] = json!("Bad Id")), "widget.json: id: use a-z"),
            (Box::new(|v| v["entry"] = json!("../../etc/passwd")), "widget.json: entry: must be a path inside the widget"),
            (Box::new(|v| v["actions"][0]["args"][0]["type"] = json!("blob")), "widget.json: actions[0].args[0].type: one of string, number, boolean"),
            (Box::new(|v| v["actions"][2]["name"] = json!("play")), "widget.json: actions[2].name: “play” is listed twice"),
            (Box::new(|v| v.as_object_mut().unwrap().remove("window").map(|_| ()).unwrap()), "widget.json: window: missing"),
            (Box::new(|v| v["actions"][0]["readOnly"] = json!("no")), "widget.json: actions[0].readOnly: not true or false"),
        ];
        for (mutate, want) in cases {
            let mut v = good();
            mutate(&mut v);
            let e = parse(&v).unwrap_err();
            assert!(e.starts_with(want), "{e} (wanted {want})");
        }
        assert!(Manifest::parse(b"{nope").unwrap_err().starts_with("widget.json: not JSON"));
    }

    #[test]
    fn cli_words_bind_to_args() {
        let m = parse(&good()).unwrap();
        assert_eq!(m.action("play").unwrap().bind("music", &["lofi".into(), "beats".into()]).unwrap(), json!({ "query": "lofi beats" }));
        assert!(m.action("play").unwrap().bind("music", &[]).unwrap_err().contains("needs <query>"));
        assert!(m.action("pause").unwrap().bind("music", &["x".into()]).unwrap_err().starts_with("too many words"));
    }
}
