//! `buddy-studio` (§4.10, frozen): the headless studio. `buddy studio <cmd> …` execs it.
//! Exit 0 ok, 1 findings or error, 2 usage. `--json` prints one JSON object instead of the
//! one-line summary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use studio::{aseprite, audit, comments, diff, frames, lint, session, Bg, Error, RenderOpts};

const USAGE: &str = "usage: buddy studio <cmd> …
  render --pack <dir> --clip <c> --dir <D|A|B> [--frame N] [--zoom K] [--eyes open|closed] [--mouth closed|open|wide] --out <png>
  sheet  --pack <dir> [--clip c] [--zoom K] [--bg none|grid|desktop] --out <png>
  gif    --pack <dir> --clip c --dir D [--zoom K] --out <gif>
  diff   <a.png> <b.png> [--out <png>]
  lint   --pack <dir>
  audit  --pack <dir> [--minutes 30] [--seed N] [--timeline t.json] --out <dir>
  comments --pack <dir> [--open]
  import-aseprite <file.aseprite> --pack <dir> --name <clip-prefix>
every command takes --json";

/// Parsed argv: positionals, `--key value` options, `--flag`s.
struct Args {
    pos: Vec<String>,
    opts: BTreeMap<String, String>,
    flags: Vec<String>,
}

const FLAGS: [&str; 2] = ["--json", "--open"];

fn parse_args(argv: &[String], allowed: &[&str]) -> Result<Args, String> {
    let mut a = Args { pos: vec![], opts: BTreeMap::new(), flags: vec![] };
    let mut i = 0;
    while i < argv.len() {
        let s = &argv[i];
        if FLAGS.contains(&s.as_str()) {
            a.flags.push(s.clone());
        } else if let Some(k) = s.strip_prefix("--") {
            if !allowed.contains(&k) {
                return Err(format!("unknown option {s}"));
            }
            let v = argv.get(i + 1).ok_or(format!("{s} needs a value"))?;
            if a.opts.insert(k.to_string(), v.clone()).is_some() {
                return Err(format!("{s} given twice"));
            }
            i += 1;
        } else {
            a.pos.push(s.clone());
        }
        i += 1;
    }
    Ok(a)
}

impl Args {
    fn req(&self, k: &str) -> Result<&str, String> {
        self.opts.get(k).map(|s| s.as_str()).ok_or(format!("--{k} is required"))
    }
    fn opt(&self, k: &str) -> Option<&str> {
        self.opts.get(k).map(|s| s.as_str())
    }
    fn num<T: std::str::FromStr>(&self, k: &str, def: T) -> Result<T, String> {
        match self.opts.get(k) {
            None => Ok(def),
            Some(v) => v.parse().map_err(|_| format!("--{k} {v}: not a number")),
        }
    }
}

/// What a command ends with.
enum Out {
    /// exit 0: the line, the JSON
    Ok(String, serde_json::Value),
    /// exit 1, but with a result (findings)
    Findings(String, serde_json::Value),
}

enum Fail {
    Usage(String),
    Error(String),
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        Fail::Error(e.0)
    }
}

fn usage<T>(m: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Usage(m.into()))
}

fn open(a: &Args) -> Result<session::Session, Fail> {
    let dir = PathBuf::from(a.req("pack").map_err(Fail::Usage)?);
    Ok(session::Session::open(&dir)?)
}

fn zoom(a: &Args, def: usize) -> Result<usize, Fail> {
    let z: usize = a.num("zoom", def).map_err(Fail::Usage)?;
    if !(1..=16).contains(&z) {
        return usage("--zoom is 1..16");
    }
    Ok(z)
}

fn ok_file(out: &Path, w: usize, h: usize, bytes: &[u8]) -> Result<Out, Fail> {
    studio::write_file(out, bytes)?;
    let sha = studio::sha256_hex(bytes);
    Ok(Out::Ok(format!("ok {} {w}×{h} sha256 {sha}", out.display()), serde_json::json!({ "out": out, "w": w, "h": h, "sha256": sha })))
}

fn run(cmd: &str, rest: &[String]) -> Result<Out, Fail> {
    let p = |allowed: &[&str]| parse_args(rest, allowed).map_err(Fail::Usage);
    match cmd {
        "render" => {
            let a = p(&["pack", "clip", "dir", "frame", "zoom", "eyes", "mouth", "out"])?;
            for k in ["pack", "clip", "dir", "out"] {
                a.req(k).map_err(Fail::Usage)?;
            }
            let eyes = match a.opt("eyes") {
                None => mascot_core::EyeState::Open,
                Some(v) => frames::parse_eyes(v).ok_or(Fail::Usage(format!("--eyes {v}: open or closed")))?,
            };
            let mouth = match a.opt("mouth") {
                None => mascot_core::MouthState::Closed,
                Some(v) => frames::parse_mouth(v).ok_or(Fail::Usage(format!("--mouth {v}: closed, open or wide")))?,
            };
            let opts = RenderOpts {
                clip: a.req("clip").map_err(Fail::Usage)?.into(),
                view: a.req("dir").map_err(Fail::Usage)?.into(),
                frame: a.num("frame", 0).map_err(Fail::Usage)?,
                zoom: zoom(&a, 1)?,
                eyes,
                mouth,
            };
            let out = PathBuf::from(a.req("out").map_err(Fail::Usage)?);
            let s = open(&a)?;
            let f = studio::render(&s.pack, &s.art, &opts)?;
            ok_file(&out, f.w, f.h, &f.png())
        }
        "sheet" => {
            let a = p(&["pack", "clip", "zoom", "bg", "out"])?;
            let bg = match a.opt("bg") {
                None => Bg::None,
                Some(v) => Bg::parse(v).ok_or(Fail::Usage(format!("--bg {v}: none, grid or desktop")))?,
            };
            let out = PathBuf::from(a.req("out").map_err(Fail::Usage)?);
            let z = zoom(&a, 1)?;
            let s = open(&a)?;
            let (f, rows) = studio::sheet(&s.pack, &s.art, a.opt("clip"), z, bg)?;
            let bytes = f.png();
            let mut r = ok_file(&out, f.w, f.h, &bytes)?;
            if let Out::Ok(_, j) = &mut r {
                j["rows"] = rows.iter().map(|r| serde_json::json!({ "clip": r.clip, "dir": r.view, "y": r.y, "frames": r.frames })).collect();
            }
            Ok(r)
        }
        "gif" => {
            let a = p(&["pack", "clip", "dir", "zoom", "out"])?;
            let out = PathBuf::from(a.req("out").map_err(Fail::Usage)?);
            let (clip, dir) = (a.req("clip").map_err(Fail::Usage)?, a.req("dir").map_err(Fail::Usage)?);
            let z = zoom(&a, 1)?;
            let s = open(&a)?;
            let bytes = studio::gif(&s.pack, &s.art, clip, dir, z)?;
            studio::write_file(&out, &bytes)?;
            let sha = studio::sha256_hex(&bytes);
            Ok(Out::Ok(format!("ok {} sha256 {sha}", out.display()), serde_json::json!({ "out": out, "sha256": sha })))
        }
        "diff" => {
            let a = p(&["out"])?;
            let [x, y] = a.pos.as_slice() else { return usage("diff takes two PNGs") };
            let ra = mascot_render::read_png(Path::new(x)).map_err(Error::from)?;
            let rb = mascot_render::read_png(Path::new(y)).map_err(Error::from)?;
            let out = a.opt("out").map(PathBuf::from);
            let (rep, overlay) = diff::diff((ra.0, ra.1, &ra.2), (rb.0, rb.1, &rb.2), out.is_some())?;
            if let (Some(o), Some(px)) = (&out, overlay) {
                studio::write_file(o, &studio::png_bytes(rep.w, rep.h, &px))?;
            }
            let j = serde_json::to_value(&rep).expect("json");
            let line = match rep.bbox {
                None => "ok identical".to_string(),
                Some([bx, by, bw, bh]) => format!("changed_px {} bbox {bx},{by} {bw}×{bh}", rep.changed_px),
            };
            Ok(if rep.changed_px == 0 { Out::Ok(line, j) } else { Out::Findings(line, j) })
        }
        "lint" => {
            let a = p(&["pack"])?;
            let dir = PathBuf::from(a.req("pack").map_err(Fail::Usage)?);
            // lint reads the strips itself: a pack whose art can't load is still linted
            let pack = mascot_core::pack::load(&dir).map_err(Error::from)?;
            let r = lint::lint(&pack);
            let mut line = r.issues.iter().map(|i| format!("{} {} {}{}: {}\n", i.severity, i.code, i.file, i.frame.map(|f| format!(" f{f}")).unwrap_or_default() + &i.x.zip(i.y).map(|(x, y)| format!(" ({x},{y})")).unwrap_or_default(), i.detail)).collect::<String>();
            line += &format!("{} {} files, {} errors, {} warnings", if r.errors == 0 { "ok" } else { "lint:" }, r.files, r.errors, r.warnings);
            let j = serde_json::to_value(&r).expect("json");
            Ok(if r.errors == 0 { Out::Ok(line, j) } else { Out::Findings(line, j) })
        }
        "audit" => {
            let a = p(&["pack", "minutes", "seed", "timeline", "out"])?;
            let out = PathBuf::from(a.req("out").map_err(Fail::Usage)?);
            let minutes: f64 = a.num("minutes", 30.0).map_err(Fail::Usage)?;
            if !(minutes > 0.0 && minutes <= 24.0 * 60.0) {
                return usage("--minutes is above 0 and at most 1440");
            }
            let seed: u64 = a.num("seed", 1).map_err(Fail::Usage)?;
            let dir = PathBuf::from(a.req("pack").map_err(Fail::Usage)?);
            let pack = std::sync::Arc::new(mascot_core::pack::load(&dir).map_err(Error::from)?);
            let tl = match a.opt("timeline") {
                None => audit::quiet(minutes, seed),
                Some(f) => {
                    let t = std::fs::read_to_string(f).map_err(|e| Fail::Error(format!("{f}: {e}")))?;
                    let mut tl = mascot_core::replay::parse(&t).map_err(|e| Fail::Error(format!("{f}: {e}")))?;
                    if a.opt("seed").is_some() {
                        tl.seed = seed;
                    }
                    if a.opt("minutes").is_some() || tl.duration_ms == 0 {
                        tl.duration_ms = (minutes * 60_000.0) as u64;
                    }
                    tl
                }
            };
            let order: Vec<String> = pack.idle_pool.iter().map(|e| e.behavior.clone()).collect();
            let r = audit::run(pack, &tl)?;
            let json = serde_json::to_string_pretty(&r).expect("json") + "\n";
            let (w, h, px) = audit::chart(&r, &order);
            let (jf, pf) = (out.join("audit.json"), out.join("audit.png"));
            studio::write_file(&jf, json.as_bytes())?;
            studio::write_file(&pf, &studio::png_bytes(w, h, &px))?;
            let j = serde_json::to_value(&r).expect("json");
            Ok(Out::Ok(format!("ok {} {}  distinct={}", jf.display(), pf.display(), r.distinct), j))
        }
        "comments" => {
            let a = p(&["pack"])?;
            let dir = PathBuf::from(a.req("pack").map_err(Fail::Usage)?);
            if !dir.join("pack.json").is_file() {
                return Err(Fail::Error(format!("{}: no pack.json here", dir.display())));
            }
            let open_only = a.flags.iter().any(|f| f == "--open");
            let all: Vec<_> = comments::read(&dir)?.into_iter().filter(|c| !open_only || c.status == "open").collect();
            let mut line: String = all.iter().map(|c| format!("{} {} {} f{} {} {}\n", c.id, c.status, c.clip, c.frame, c.dir, c.text.replace('\n', " "))).collect();
            line += &format!("ok {} comments", all.len());
            Ok(Out::Ok(line, serde_json::json!({ "comments": all })))
        }
        "import-aseprite" => {
            let a = p(&["pack", "name"])?;
            let [file] = a.pos.as_slice() else { return usage("import-aseprite takes one .aseprite file") };
            let (pack, name) = (a.req("pack").map_err(Fail::Usage)?, a.req("name").map_err(Fail::Usage)?);
            let r = aseprite::import(Path::new(file), Path::new(pack), name)?;
            let line = format!("ok {} clips {} ({} files{})", r.pack, r.clips.join(","), r.files.len(), if r.created_pack { ", new pack" } else { "" });
            Ok(Out::Ok(line, serde_json::to_value(&r).expect("json")))
        }
        _ => usage(format!("unknown command “{cmd}”")),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if cmd == "-h" || cmd == "--help" || cmd == "help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let rest = &argv[1..];
    let json = rest.iter().any(|a| a == "--json");
    match run(&cmd, rest) {
        Ok(Out::Ok(line, j)) => {
            println!("{}", if json { j.to_string() } else { line });
            ExitCode::SUCCESS
        }
        Ok(Out::Findings(line, j)) => {
            println!("{}", if json { j.to_string() } else { line });
            ExitCode::from(1)
        }
        Err(Fail::Usage(m)) => {
            eprintln!("{cmd}: {m}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Fail::Error(m)) => {
            if json {
                println!("{}", serde_json::json!({ "error": m }));
            }
            eprintln!("{cmd}: {m}");
            ExitCode::from(1)
        }
    }
}
