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
//! Vectors (p3): every tick, while memory is on and `settings.memory.embed` is a local URL, [`vectors_once`]
//! probes the embedder (lists the model in 2 s, then one warm-up embed so the first message isn't a cold load;
//! at start, every 10 min, and on the tick after an unreachable or slow call; a wrong-sized or refused reply
//! waits for the 10 min probe, so a bad model doesn't retry every tick) and, while it
//! answers, embeds up to [`BACKFILL`] rows without a vector (one call, ≤ 10 s, no store held) and stores
//! them (`put_vectors`: dropped if the model changed or the row went meanwhile). A new model drops every
//! vector first. No model, no module (`vectors_on`), or Ollama down: recall stays keyword only.
//!
//! Logs: `[buddy] memory vectors on|off`, `[buddy] memory vectors embedded <n>`, `[buddy] memory vectors failed (<why>)`,
//! `[buddy] memory vectors reset (model changed, <n> dropped)`, `[buddy] memory extract <n> turns → <m> items`, `[buddy] memory <outcome> <id>`,
//! `[buddy] memory extract failed (<code>)`, `[buddy] memory drop <request_id>`,
//! `[buddy] memory reflect <batch8> done|failed|rejected (<pct>% changed)`. Never text.

use std::sync::Arc;
use std::time::Duration;

use memory::embed::Embedder;
use memory::Store;

use super::{now, werr, Mem, Vectors, BACKFILL, BACKFILL_WAIT, PROBE_WAIT};

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

/// Sidecar error codes that mean the model was never asked, so the batch's try is given back (a sidecar
/// killed mid-call reaches the job as `not_running`).
const UNSPENT: [&str; 2] = ["no_provider", "not_running"];

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
            // A call the model never saw (signed out, the sidecar stopped or was killed) gives the try back;
            // one that reached it and failed (`agent_error`: bad output, a model timeout or error; or the
            // bridge's own `timeout`) uses it up.
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

/// The vector look (p3, every tick): the probe when due (at start, every 10 min, and after a failure),
/// then one backfill batch of rows without a vector. Never on the prompt path. Returns how many it stored.
pub fn vectors_once(m: &Mem) -> usize {
    let v = m.vectors();
    let Some(e) = v.embedder.clone() else { return 0 };
    if !m.enabled() {
        return 0;
    }
    let Some((store, gen)) = m.store_gen() else { return 0 };
    if !store.vectors_on() {
        v.set(false);
        return 0;
    }
    if v.probe_due() {
        drop(store);
        // listed, then loaded (a cold model would miss the first message's 500 ms)
        let ok = e.probe(PROBE_WAIT) && e.warm(BACKFILL_WAIT);
        v.probed();
        v.set(ok);
        if !ok {
            return 0;
        }
        let Some((s, g)) = m.store_gen() else { return 0 };
        if g != gen {
            return 0;
        }
        return backfill(m, &v, &e, s, g);
    }
    if !v.up() {
        return 0;
    }
    backfill(m, &v, &e, store, gen)
}

fn backfill(m: &Mem, v: &Vectors, e: &Embedder, store: Arc<Store>, gen: u64) -> usize {
    let had = store.vector_count();
    match store.set_embed_model(e.model()) {
        Ok(true) if had > 0 => eprintln!("[buddy] memory vectors reset (model changed, {had} dropped)"),
        Ok(_) => {}
        Err(err) => {
            eprintln!("[buddy] memory vectors not kept: {}", werr(&err));
            return 0;
        }
    }
    let todo = match store.vectors_todo(BACKFILL) {
        Ok(t) if !t.is_empty() => t,
        _ => return 0,
    };
    drop(store);
    let texts: Vec<String> = todo.iter().map(|(_, t)| t.clone()).collect();
    let vecs = match e.documents(&texts, BACKFILL_WAIT) {
        Ok(x) => x,
        Err(why) => {
            eprintln!("[buddy] memory vectors failed ({why})");
            v.failed(why);
            return 0;
        }
    };
    let Some((store, g)) = m.store_gen() else { return 0 };
    if g != gen || !m.enabled() {
        return 0;
    }
    let rows: Vec<(i64, String, Vec<f32>)> = todo.into_iter().zip(vecs).map(|((id, t), v)| (id, t, v)).collect();
    match store.put_vectors(e.model(), &rows) {
        Ok(n) => {
            eprintln!("[buddy] memory vectors embedded {n}");
            n
        }
        Err(err) => {
            eprintln!("[buddy] memory vectors not kept: {}", werr(&err));
            0
        }
    }
}

/// Start the worker (with the harness). It lives as long as the app.
pub fn start(m: Arc<Mem>) {
    let idle = idle();
    if idle != IDLE {
        eprintln!("[buddy] memory idle {} s", idle.as_secs());
    }
    let spawned = std::thread::Builder::new().name("buddy-memory".into()).spawn(move || {
        let mut last_reflect = std::time::Instant::now();
        // the first probe at start, not 30 s in
        vectors_once(&m);
        loop {
            std::thread::sleep(TICK);
            run_once(&m, idle);
            vectors_once(&m);
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
