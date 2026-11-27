//! The memory worker thread (design §1.6): every 30 s, when memory is on and the store is open, the
//! agent is running with no turn busy, turns are pending, and the chat has been quiet for
//! `BUDDY_MEMORY_IDLE_S` (default 300 s) or 10 turns wait, it takes the ≤ 10 oldest pending turns,
//! asks the sidecar's extractor (`memory.extract`), and applies the reply (`apply_extract`) or counts a
//! failure (`extract_failed`, which drops a turn after 3 tries or 7 days). Then the usual change:
//! MEMORY.md, the agent's section, the window's state.
//!
//! Reflection (p2, supplied § Reflection job): every [`REFLECT_EVERY`] the same thread looks again; a
//! run starts when memory is on, the person has been away from the keyboard for [`REFLECT_IDLE`] or
//! more, no turn is busy, the agent runs, and the store says one is due (`reflect_due`: nothing pending,
//! and enough importance since the last run, or a day and a new episode). It takes the batch
//! (`reflect_args`), asks the sidecar's `memory.reflect` (one model call, ≤ 125 s) and applies the reply
//! in one transaction (`apply_reflect`, the 30% guard and retention inside), or marks the run failed.
//!
//! Logs: `[buddy] memory extract <n> turns → <m> items`, `[buddy] memory <outcome> <id>`,
//! `[buddy] memory extract failed (<code>)`, `[buddy] memory drop <request_id>`,
//! `[buddy] memory reflect <batch8> done|failed|rejected (<pct>% changed)`. Never text.

use std::sync::Arc;
use std::time::Duration;

use super::{now, werr, Mem};

/// How often the worker looks.
pub const TICK: Duration = Duration::from_secs(30);
/// The most pending turns one extraction takes (and the count that skips the idle wait).
pub const BATCH: usize = 10;
/// The default quiet time after the last chat message.
pub const IDLE: Duration = Duration::from_secs(300);
/// How often reflection looks.
pub const REFLECT_EVERY: Duration = Duration::from_secs(600);
/// How long the person must have been away from keyboard and mouse before a reflection runs.
pub const REFLECT_IDLE: Duration = Duration::from_secs(30 * 60);

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
    /// A reflection ran: `done`, `failed` or `rejected`, with the share of live rows it changed.
    Reflected { batch8: String, outcome: &'static str, pct: u32 },
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

/// Sidecar error codes that mean the model was never asked, so the batch's try is given back.
const UNSPENT: [&str; 3] = ["no_provider", "not_running", "exited"];

/// One reflection look (every [`REFLECT_EVERY`]; tests call it directly with a fake sidecar).
/// `user_idle` is the time since the person's last input.
pub fn reflect_once(m: &Mem, user_idle: Duration) -> Ran {
    if !m.enabled() {
        return Ran::Waiting("off");
    }
    if user_idle < REFLECT_IDLE {
        return Ran::Waiting("person here");
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
    if !store.reflect_due(&now) {
        return Ran::Waiting("not due");
    }
    let Some(job) = store.reflect_args(now.today, &now) else { return Ran::Waiting("nothing new") };
    let batch8: String = job.batch_id.chars().take(8).collect();
    // one model call of up to two minutes: hold no store meanwhile (Forget everything may replace it)
    drop(store);
    let reply = side.reflect(job.args.clone());
    let Some((store, now_gen)) = m.store_gen() else { return Ran::Waiting("no store") };
    let now = super::now();
    if now_gen != gen {
        // erased meanwhile: the batch is gone with the store
        return Ran::Waiting("changed");
    }
    if !m.enabled() {
        // switched off while the model ran: drop the reply, and the try with it
        let _ = store.reflect_unspent(&job, &now);
        return Ran::Waiting("changed");
    }
    let (outcome, pct) = match reply {
        Err(code) => {
            eprintln!("[buddy] memory reflect {batch8} failed ({code})");
            // A reply that never came (signed out, the sidecar stopped or was killed) gives the try back;
            // one that came and was unusable (bad_output, timeout, model_error) uses it up.
            if UNSPENT.contains(&code.as_str()) {
                let _ = store.reflect_unspent(&job, &now);
            } else {
                let _ = store.reflect_failed(&job, &now);
            }
            ("failed", 0)
        }
        Ok(r) => match store.apply_reflect(&job, &r, &now) {
            Ok(done) => {
                eprintln!("[buddy] memory reflect {batch8} done ({}% changed)", done.pct);
                ("done", done.pct)
            }
            Err(memory::Error::Rejected { pct }) => {
                eprintln!("[buddy] memory reflect {batch8} rejected ({pct}% changed)");
                ("rejected", pct)
            }
            Err(e) => {
                eprintln!("[buddy] memory reflect {batch8} failed ({})", werr(&e));
                let _ = store.reflect_failed(&job, &now);
                ("failed", 0)
            }
        },
    };
    drop(store);
    m.changed();
    Ran::Reflected { batch8, outcome, pct }
}

/// Start the worker (with the harness). It lives as long as the app.
pub fn start(m: Arc<Mem>) {
    let idle = idle();
    if idle != IDLE {
        eprintln!("[buddy] memory idle {} s", idle.as_secs());
    }
    let spawned = std::thread::Builder::new().name("buddy-memory".into()).spawn(move || {
        let mut last_reflect = std::time::Instant::now();
        loop {
            std::thread::sleep(TICK);
            run_once(&m, idle);
            if last_reflect.elapsed() >= REFLECT_EVERY {
                last_reflect = std::time::Instant::now();
                reflect_once(&m, Duration::from_millis(crate::senses::idle::idle_ms()));
            }
        }
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
