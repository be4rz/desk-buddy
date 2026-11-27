//! The memory worker thread (design §1.6): every 30 s, when memory is on and the store is open, the
//! agent is running with no turn busy, turns are pending, and the chat has been quiet for
//! `BUDDY_MEMORY_IDLE_S` (default 300 s) or 10 turns wait, it takes the ≤ 10 oldest pending turns,
//! asks the sidecar's extractor (`memory.extract`), and applies the reply (`apply_extract`) or counts a
//! failure (`extract_failed`, which drops a turn after 3 tries or 7 days). Then the usual change:
//! MEMORY.md, the agent's section, the window's state.
//!
//! Logs: `[buddy] memory extract <n> turns → <m> items`, `[buddy] memory <outcome> <id>`,
//! `[buddy] memory extract failed (<code>)`, `[buddy] memory drop <request_id>`. Never text.

use std::sync::Arc;
use std::time::Duration;

use super::{now, werr, Mem};

/// How often the worker looks.
pub const TICK: Duration = Duration::from_secs(30);
/// The most pending turns one extraction takes (and the count that skips the idle wait).
pub const BATCH: usize = 10;
/// The default quiet time after the last chat message.
pub const IDLE: Duration = Duration::from_secs(300);

/// `BUDDY_MEMORY_IDLE_S`, else 300 s.
pub fn idle() -> Duration {
    parse_idle(std::env::var("BUDDY_MEMORY_IDLE_S").ok().as_deref())
}

fn parse_idle(v: Option<&str>) -> Duration {
    v.and_then(|s| s.trim().parse::<u64>().ok()).map_or(IDLE, Duration::from_secs)
}

/// What one look did.
#[derive(Debug, PartialEq, Eq)]
pub enum Ran {
    /// Nothing ran, and why (a short word).
    Waiting(&'static str),
    /// `turns` were sent and the reply applied with `items` items.
    Extracted { turns: usize, items: usize },
    /// The extractor (or applying its reply) failed; these turns were dropped.
    Failed { turns: usize, dropped: Vec<String> },
}

/// One look (the tick's body; tests call it directly with a fake sidecar).
pub fn run_once(m: &Mem, idle: Duration) -> Ran {
    if !m.enabled() {
        return Ran::Waiting("off");
    }
    let Some(side) = m.sidecar() else { return Ran::Waiting("no agent") };
    if !side.running() {
        return Ran::Waiting("no agent");
    }
    if m.busy() {
        return Ran::Waiting("busy");
    }
    let Some((store, gen)) = m.store_gen() else { return Ran::Waiting("no store") };
    let now = now();
    let Some(args) = store.extract_args(BATCH, now.today) else { return Ran::Waiting("nothing pending") };
    let turns = args["turns"].as_array().map_or(0, Vec::len);
    if turns == 0 {
        return Ran::Waiting("nothing pending");
    }
    if turns < BATCH && m.since_chat() < idle {
        return Ran::Waiting("chatting");
    }
    // the extractor can take a minute: hold no store meanwhile (Forget everything may replace it)
    drop(store);
    let reply = side.extract(args.clone());
    let Some((store, now_gen)) = m.store_gen() else { return Ran::Waiting("no store") };
    if now_gen != gen || !m.enabled() {
        // erased or switched off while the model ran: the reply belongs to nothing now
        return Ran::Waiting("changed");
    }
    let applied = reply.map_err(|code| format!("({code})")).and_then(|r| store.apply_extract(&args, &r, &now).map_err(|e| format!("({})", werr(&e))));
    let ran = match applied {
        Ok(a) => {
            eprintln!("[buddy] memory extract {turns} turns → {} items", a.items.len());
            for it in &a.items {
                if let Some(id) = it.outcome.id() {
                    eprintln!("[buddy] memory {} {id}", it.outcome.name());
                }
            }
            Ran::Extracted { turns, items: a.items.len() }
        }
        Err(why) => {
            eprintln!("[buddy] memory extract failed {why}");
            let dropped = store.extract_failed(&args).unwrap_or_default();
            for id in &dropped {
                eprintln!("[buddy] memory drop {id}");
            }
            Ran::Failed { turns, dropped }
        }
    };
    drop(store);
    m.changed();
    ran
}

/// Start the worker (with the harness). It lives as long as the app.
pub fn start(m: Arc<Mem>) {
    let idle = idle();
    if idle != IDLE {
        eprintln!("[buddy] memory idle {} s", idle.as_secs());
    }
    let spawned = std::thread::Builder::new().name("buddy-memory".into()).spawn(move || loop {
        std::thread::sleep(TICK);
        run_once(&m, idle);
    });
    if let Err(e) = spawned {
        eprintln!("[buddy] memory worker didn't start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idle_wait_is_300_s_unless_overridden() {
        assert_eq!(parse_idle(None), IDLE);
        assert_eq!(parse_idle(Some("1")), Duration::from_secs(1));
        assert_eq!(parse_idle(Some("soon")), IDLE);
    }
}
