//! `buddy memory …` (design I4): the CLI side. Request building and the one-line fact text live in
//! `host::memory`; this runs them against the API. Exits: 0 ok, 1 refused or unreachable, 2 usage.

use std::process::ExitCode;

use host::memory::{cli_forget, cli_list, cli_remember, cli_search, fact_line};
use serde_json::json;

use super::{call, finish, usage};

const USAGE: &str = "buddy memory remember \"<text>\" [--key pred[:qualifier]] [--value V] [--kind K] [--importance N] [--from DATE] | forget <id> | forget --key pred[:qualifier] | list [--kind K] | search \"<words>\" [--limit N] | open";

/// The facts a list or search answered, one line each, or `empty`.
fn facts(r: Result<host::client::Reply, String>, json_out: bool, empty: &str) -> ExitCode {
    match r {
        Ok(r) if r.status == 200 && !json_out => {
            let list = r.body["facts"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("{empty}");
            }
            for f in &list {
                println!("{}", fact_line(f));
            }
            ExitCode::SUCCESS
        }
        other => finish(other, json_out, ""),
    }
}

pub fn run(rest: &[&str], json_out: bool) -> ExitCode {
    match rest {
        ["remember", args @ ..] => match cli_remember(args) {
            Err(e) => usage(&e),
            Ok(body) => match call("POST", "/v1/memory/remember", Some(body)) {
                Ok(r) if r.status == 200 && !json_out => {
                    println!("#{} {}", r.body["id"], r.body["outcome"].as_str().unwrap_or("done"));
                    ExitCode::SUCCESS
                }
                other => finish(other, json_out, ""),
            },
        },
        ["forget", args @ ..] => match cli_forget(args) {
            Err(e) => usage(&e),
            Ok(body) => match call("POST", "/v1/memory/forget", Some(body)) {
                Ok(r) if r.status == 200 && !json_out => {
                    let ids: Vec<String> = r.body["forgotten"].as_array().into_iter().flatten().map(|i| format!("#{i}")).collect();
                    println!("forgot {}", if ids.is_empty() { "nothing".to_string() } else { ids.join(" ") });
                    ExitCode::SUCCESS
                }
                other => finish(other, json_out, ""),
            },
        },
        ["list" | "ls", args @ ..] => match cli_list(args) {
            Err(e) => usage(&e),
            Ok(path) => facts(call("GET", &path, None), json_out, "nothing remembered yet"),
        },
        ["search", args @ ..] => match cli_search(args) {
            Err(e) => usage(&e),
            Ok(path) => facts(call("GET", &path, None), json_out, "no matches"),
        },
        ["open"] => finish(call("POST", "/v1/memory/open", Some(json!({}))), json_out, "opened"),
        _ => usage(&format!("usage: {USAGE}")),
    }
}

