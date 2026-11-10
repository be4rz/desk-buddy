//! `mascot-replay --pack <dir> --timeline <file.json> [--seed N]`: prints the
//! brain's decisions for a scripted timeline as JSONL. Exit 2 on usage, 1 on a
//! pack or timeline error (one line on stderr).

use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;

use mascot_core::{pack, replay};

const USAGE: &str = "usage: mascot-replay --pack <dir> --timeline <file.json> [--seed N]";

fn main() -> ExitCode {
    let mut pack_dir = None;
    let mut timeline = None;
    let mut seed = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--pack", Some(v)) => pack_dir = Some(v),
            ("--timeline", Some(v)) => timeline = Some(v),
            ("--seed", Some(v)) => match v.parse::<u64>() {
                Ok(n) => seed = Some(n),
                Err(_) => return usage(&format!("--seed {v:?} is not a number")),
            },
            ("-h" | "--help", _) => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            (other, _) => return usage(&format!("unexpected argument {other:?}")),
        }
    }
    let (Some(pack_dir), Some(timeline)) = (pack_dir, timeline) else { return usage("--pack and --timeline are required") };

    let pack = match pack::load(pack_dir.as_ref()) {
        Ok(p) => Arc::new(p),
        Err(e) => return fail(&e.to_string()),
    };
    let text = match std::fs::read_to_string(&timeline) {
        Ok(t) => t,
        Err(e) => return fail(&format!("timeline {timeline}: {e}")),
    };
    let mut tl = match replay::parse(&text) {
        Ok(t) => t,
        Err(e) => return fail(&format!("timeline {timeline}: {e}")),
    };
    if let Some(s) = seed {
        tl.seed = s;
    }
    let out = replay::to_jsonl(&replay::run(pack, &tl));
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(out.as_bytes()).and_then(|_| stdout.flush()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("mascot-replay: {msg}\n{USAGE}");
    ExitCode::from(2)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("mascot-replay: {msg}");
    ExitCode::from(1)
}
