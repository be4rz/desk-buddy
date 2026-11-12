//! `audit`: run the brain on a quiet timeline (or a given one) for N minutes, the same loop
//! as `mascot_core::replay::run`, and count which behaviors start. Seeded, so reproducible.

use std::collections::BTreeMap;
use std::sync::Arc;

use mascot_core::replay::{start_minutes, Timeline};
use mascot_core::{Brain, BrainConfig, DndAction, LocalTime, PackMeta, Senses};
use mascot_protocol::pack::{PoolKind, Temperament};
use serde::Serialize;

use crate::{err, Result};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Audit {
    pub pack: String,
    pub minutes: f64,
    pub seed: u64,
    /// Behavior → how many times it started.
    pub plays: BTreeMap<String, u64>,
    pub starts: u64,
    pub distinct: usize,
    /// The longest run of the same behavior starting back to back.
    pub max_same_in_row: u64,
    /// Shannon entropy of `plays`, in bits, rounded to 3 decimals.
    pub entropy: f64,
    /// Start order (behavior names), for the chart and for review.
    #[serde(skip)]
    pub sequence: Vec<String>,
}

/// A quiet day: no events, local time 10:00 Monday.
pub fn quiet(minutes: f64, seed: u64) -> Timeline {
    Timeline { timeline_version: 1, seed, duration_ms: (minutes * 60_000.0) as u64, start_local: "10:00".into(), weekday: 0, items: vec![] }
}


pub fn run(pack: Arc<PackMeta>, tl: &Timeline) -> Result<Audit> {
    if tl.duration_ms == 0 {
        return err("the audit needs a duration above 0");
    }
    let id = pack.id.clone();
    let mut brain = Brain::new(pack, BrainConfig { seed: tl.seed, temperament: None, dnd_action: DndAction::Hide }, 0);
    let mut senses = Senses { local: LocalTime { minutes: start_minutes(&tl.start_local).map_err(crate::Error)?, weekday: tl.weekday }, ..Senses::default() };
    brain.senses(&senses, 0);
    let mut seq = vec![];
    let mut done: Option<u64> = None;
    let (mut i, mut t) = (0, 0u64);
    let mut take = |brain: &Brain, upto: u64, done: &mut Option<u64>| {
        for l in brain.trace() {
            if done.is_none_or(|d| l.t_ms > d) && l.t_ms <= upto && l.what == "start" {
                seq.push(l.behavior.clone());
            }
        }
        *done = Some(upto);
    };
    loop {
        while i < tl.items.len() && tl.items[i].at_ms <= t {
            let it = &tl.items[i];
            if let Some(p) = &it.senses {
                p.apply(&mut senses);
                brain.senses(&senses, t);
            }
            if let Some(e) = &it.event {
                brain.event(e, t);
            }
            if let Some(p) = it.poke {
                brain.poke(p, t);
            }
            i += 1;
        }
        brain.tick(t);
        take(&brain, t, &mut done);
        let next_item = tl.items.get(i).map_or(u64::MAX, |it| it.at_ms);
        let next = next_item.min(brain.next_wake_ms()).max(t + 1);
        if next > tl.duration_ms {
            break;
        }
        t = next;
    }
    let mut plays: BTreeMap<String, u64> = BTreeMap::new();
    for b in &seq {
        *plays.entry(b.clone()).or_default() += 1;
    }
    let (mut best, mut run) = (0u64, 0u64);
    for (k, b) in seq.iter().enumerate() {
        run = if k > 0 && seq[k - 1] == *b { run + 1 } else { 1 };
        best = best.max(run);
    }
    let total = seq.len() as f64;
    let h: f64 = plays.values().map(|&n| n as f64 / total).map(|p| -p * p.log2()).sum();
    Ok(Audit {
        pack: id,
        minutes: tl.duration_ms as f64 / 60_000.0,
        seed: tl.seed,
        distinct: plays.len(),
        starts: seq.len() as u64,
        plays,
        max_same_in_row: best,
        entropy: if total > 0.0 { (h * 1000.0).round() / 1000.0 } else { 0.0 },
        sequence: seq,
    })
}

/// The bar chart (`audit.png`): one bar per behavior, idle pool order first, height ∝ plays,
/// under it a strip of the start sequence coloured per behavior. Straight RGBA.
pub fn chart(a: &Audit, order: &[String]) -> (usize, usize, Vec<u8>) {
    let mut names: Vec<&String> = order.iter().filter(|b| a.plays.contains_key(*b)).collect();
    for b in a.plays.keys() {
        if !names.contains(&b) {
            names.push(b);
        }
    }
    // muted ramp colours from the scene palette (teal, plum, moss, sand, rose, sky…)
    const COLS: [[u8; 3]; 8] = [[0x4f, 0x8a, 0x83], [0x8b, 0x78, 0xad], [0x55, 0x6b, 0x5a], [0xc2, 0xa0, 0x6e], [0xc0, 0x6c, 0x84], [0x5b, 0x7f, 0xb0], [0x9a, 0x8f, 0x7a], [0x6c, 0x5a, 0x7e]];
    let (bar, gap, top, hgt, strip) = (28usize, 12usize, 10usize, 120usize, 10usize);
    let w = (names.len().max(1) * (bar + gap) + gap).max(200);
    let h = top + hgt + 10 + strip + 10;
    let mut px = vec![0u8; w * h * 4];
    let put = |px: &mut Vec<u8>, x: usize, y: usize, c: [u8; 3]| {
        let i = (y * w + x) * 4;
        px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    };
    for y in 0..h {
        for x in 0..w {
            put(&mut px, x, y, [0xf4, 0xf1, 0xea]);
        }
    }
    let max = a.plays.values().copied().max().unwrap_or(1).max(1);
    for (k, b) in names.iter().enumerate() {
        let n = a.plays[*b];
        let bh = ((n as f64 / max as f64) * hgt as f64).round() as usize;
        let x0 = gap + k * (bar + gap);
        for y in top + hgt - bh..top + hgt {
            for x in x0..x0 + bar {
                put(&mut px, x, y, COLS[k % COLS.len()]);
            }
        }
    }
    for x in gap..w - gap {
        put(&mut px, x, top + hgt, [0x2a, 0x25, 0x30]);
    }
    // the sequence strip, scaled to the width
    if !a.sequence.is_empty() {
        let y0 = top + hgt + 10;
        let span = w - 2 * gap;
        for x in 0..span {
            let s = &a.sequence[x * a.sequence.len() / span];
            let k = names.iter().position(|b| *b == s).unwrap_or(0);
            for y in y0..y0 + strip {
                put(&mut px, gap + x, y, COLS[k % COLS.len()]);
            }
        }
    }
    (w, h, px)
}

// ---- audit v2 (`#audit`): time per clip class, picks against the pack's variety rules ----

/// One run's options.
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub minutes: f64,
    pub seed: u64,
    /// `YYYY-MM-DDTHH:MM` local.
    pub start: String,
    pub temperament: Option<Temperament>,
}

pub const DEFAULT_START: &str = "2026-11-16T10:00";

/// `--start`: (minutes of the day, weekday 0 = Monday, month 1..=12).
pub fn parse_start(s: &str) -> std::result::Result<(u16, u8, u8), String> {
    let bad = || format!("--start {s}: use YYYY-MM-DDTHH:MM");
    let (date, time) = s.split_once('T').ok_or_else(bad)?;
    let d: Vec<&str> = date.split('-').collect();
    let [y, m, day] = d.as_slice() else { return Err(bad()) };
    let (y, m, day): (i64, u32, u32) = (y.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?, day.parse().map_err(|_| bad())?);
    if !(1..=12).contains(&m) || !(1970..=9999).contains(&y) {
        return Err(bad());
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m as usize - 1];
    if !(1..=month_days).contains(&day) {
        return Err(bad());
    }
    let minutes = start_minutes(time).map_err(|_| bad())?;
    // days from 1970-01-01 (a Thursday), Howard Hinnant's days_from_civil
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = (days + 3).rem_euclid(7) as u8; // 0 = Monday
    Ok((minutes, weekday, m as u8))
}

/// `--temperament energy=0.5,sleepiness=0.2` over the pack's own.
pub fn parse_temperament(base: &Temperament, s: &str) -> std::result::Result<Temperament, String> {
    let mut t = base.clone();
    for kv in s.split(',').filter(|x| !x.is_empty()) {
        let (k, v) = kv.split_once('=').ok_or(format!("--temperament {kv}: use name=0..1"))?;
        let v: f64 = v.parse().ok().filter(|v: &f64| (0.0..=1.0).contains(v)).ok_or(format!("--temperament {kv}: a value 0..1"))?;
        match k {
            "energy" => t.energy = v,
            "curiosity" => t.curiosity = v,
            "shyness" => t.shyness = v,
            "chattiness" => t.chattiness = v,
            "sleepiness" => t.sleepiness = v,
            _ => return Err(format!("--temperament {k}: one of energy, curiosity, shyness, chattiness, sleepiness")),
        }
    }
    Ok(t)
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Report {
    pub pack: String,
    pub minutes: f64,
    pub seeds: Vec<u64>,
    pub pass: bool,
    pub runs: Vec<Run>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Run {
    pub seed: u64,
    pub start: String,
    pub picks: u64,
    pub distinct: usize,
    pub relaxed: u64,
    pub sequence: Vec<String>,
    pub time: TimeShare,
    pub behaviors: BTreeMap<String, BehaviorStat>,
    pub kinds: BTreeMap<String, u64>,
    pub long: Vec<LongPlay>,
    pub violations: Vec<Violation>,
    pub targets: BTreeMap<String, Target>,
    pub pass: bool,
}

/// Shares of shown time (sum 1): `stand` = clip idle, `walk_run` = Stage A walk*/run*,
/// `stand_move` = their sum, `turn` = turn clips, `other` = the rest.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct TimeShare {
    pub stand: f64,
    pub walk_run: f64,
    pub stand_move: f64,
    pub turn: f64,
    pub other: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct BehaviorStat {
    pub plays: u64,
    pub ms: u64,
    pub share_picks: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_s: Option<Vec<f64>>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LongPlay {
    pub name: String,
    pub len_s: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Violation {
    pub rule: String,
    pub at_ms: u64,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Target {
    pub value: serde_json::Value,
    pub limit: serde_json::Value,
    pub pass: bool,
}

const STAGE_A_GAITS: [&str; 6] = ["walk_start", "walk", "walk_stop", "run_start", "run", "run_stop"];

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Every seed's run, and whether all pass.
pub fn report(pack: Arc<PackMeta>, label: &str, seeds: &[u64], minutes: f64, start: &str, temperament: Option<Temperament>) -> Result<Report> {
    let mut runs = vec![];
    for &seed in seeds {
        runs.push(run_v2(pack.clone(), &Opts { minutes, seed, start: start.to_string(), temperament: temperament.clone() })?);
    }
    Ok(Report { pack: label.to_string(), minutes, seeds: seeds.to_vec(), pass: runs.iter().all(|r| r.pass), runs })
}

/// One seeded quiet run of `minutes` from `start` (no events), measured (`#audit`).
pub fn run_v2(pack: Arc<PackMeta>, o: &Opts) -> Result<Run> {
    if !(o.minutes > 0.0) {
        return err("the audit needs a duration above 0");
    }
    let (minutes, weekday, month) = parse_start(&o.start).map_err(crate::Error)?;
    let dur = (o.minutes * 60_000.0) as u64;
    let mut brain = Brain::new(pack.clone(), BrainConfig { seed: o.seed, temperament: o.temperament.clone(), dnd_action: DndAction::Hide }, 0);
    brain.senses(&Senses { local: LocalTime { minutes, weekday }, month, ..Senses::default() }, 0);
    let turn_clips: Vec<&str> = pack.turning.iter().flat_map(|t| [t.ccw.as_str(), t.cw.as_str()]).collect();
    let (mut stand, mut gait, mut turn, mut all) = (0u64, 0u64, 0u64, 0u64);
    let mut ms: BTreeMap<String, u64> = BTreeMap::new();
    // contiguous idle segments per behavior: (name, from, to)
    let mut segs: Vec<(String, u64, u64)> = vec![];
    let mut t = 0u64;
    let mut show = brain.tick(0);
    // the brain keeps only its last 256 trace lines: take them as they come
    let mut trace = brain.drain_trace();
    loop {
        let next = brain.next_wake_ms().max(t + 1).min(dur);
        let dt = next - t;
        all += dt;
        if show.clip == "idle" {
            stand += dt;
        } else if STAGE_A_GAITS.contains(&show.clip.as_str()) {
            gait += dt;
        } else if turn_clips.contains(&show.clip.as_str()) {
            turn += dt;
        }
        *ms.entry(show.behavior.clone()).or_default() += dt;
        match segs.last_mut() {
            Some((b, _, to)) if *b == show.behavior && *to == t => *to = next,
            _ => segs.push((show.behavior.clone(), t, next)),
        }
        if next >= dur {
            break;
        }
        t = next;
        show = brain.tick(t);
        trace.extend(brain.drain_trace());
    }
    // picks, from the trace (`pick <name> total <w>`, `pick relaxed <rule>`)
    let mut seq: Vec<(u64, String)> = vec![];
    let mut relaxed: Vec<(u64, String)> = vec![];
    for l in &trace {
        if l.what != "pick" {
            continue;
        }
        if let Some(rule) = l.note.strip_prefix("relaxed ") {
            relaxed.push((l.t_ms, rule.to_string()));
        } else if let Some((name, _)) = l.note.split_once(" total ") {
            seq.push((l.t_ms, name.to_string()));
        }
    }
    let entry = |name: &str| pack.idle_pool.iter().find(|e| e.name() == name);
    let group = |name: &str| entry(name).map_or(name.to_string(), |e| e.group_key().to_string());
    let kind = |name: &str| entry(name).map_or(PoolKind::Other, |e| e.kind);
    let picks = seq.len() as u64;
    let mut behaviors: BTreeMap<String, BehaviorStat> = BTreeMap::new();
    let mut kinds: BTreeMap<String, u64> = PoolKind::ALL.iter().filter(|k| !k.is_other()).map(|k| (k.as_str().to_string(), 0)).collect();
    for (_, n) in &seq {
        behaviors.entry(n.clone()).or_default().plays += 1;
        let k = kind(n);
        if !k.is_other() {
            *kinds.entry(k.as_str().to_string()).or_default() += 1;
        }
    }
    // long plays: whole segments of a long-kind pick (one still running at the end is left out)
    let mut long = vec![];
    for (b, from, to) in &segs {
        if behaviors.contains_key(b) && kind(b) == PoolKind::Long && *to < dur {
            long.push(LongPlay { name: b.clone(), len_s: r1((to - from) as f64 / 1000.0) });
        }
    }
    for (name, st) in behaviors.iter_mut() {
        st.ms = ms.get(name).copied().unwrap_or(0);
        st.share_picks = if picks > 0 { r3(st.plays as f64 / picks as f64) } else { 0.0 };
        if kind(name) == PoolKind::Long {
            st.lens_s = Some(long.iter().filter(|l| l.name == *name).map(|l| l.len_s).collect());
        }
    }
    let share = |x: u64| if all > 0 { r3(x as f64 / all as f64) } else { 0.0 };
    let (s, w, tu) = (share(stand), share(gait), share(turn));
    let time = TimeShare { stand: s, walk_run: w, stand_move: r3(s + w), turn: tu, other: r3(1.0 - s - w - tu) };
    let mut violations = vec![];
    let mut targets = BTreeMap::new();
    if let Some(v) = &pack.variety {
        let groups: Vec<String> = seq.iter().map(|(_, n)| group(n)).collect();
        for k in 0..seq.len() {
            let at = seq[k].0;
            if v.no_repeat && k > 0 && groups[k] == groups[k - 1] {
                violations.push(Violation { rule: "repeat".into(), at_ms: at, detail: format!("{} twice in a row", groups[k]) });
            }
            if v.max_in_window > 0 && v.window > 0 {
                let from = (k + 1).saturating_sub(v.window as usize);
                let n = groups[from..=k].iter().filter(|g| **g == groups[k]).count();
                if n > v.max_in_window as usize {
                    violations.push(Violation { rule: "max_in_window".into(), at_ms: at, detail: format!("{} {n} in last {}", groups[k], v.window) });
                }
            }
            if v.max_moves_in_row > 0 {
                let run = seq[..=k].iter().rev().take_while(|(_, n)| kind(n) == PoolKind::Move).count();
                if run > v.max_moves_in_row as usize {
                    violations.push(Violation { rule: "max_moves_in_row".into(), at_ms: at, detail: format!("{run} moves in a row") });
                }
            }
        }
        for (at, rule) in &relaxed {
            violations.push(Violation { rule: "relaxed".into(), at_ms: *at, detail: format!("pick relaxed {rule}") });
        }
        violations.sort_by_key(|x| x.at_ms);
        if let Some(tg) = v.targets.as_ref().filter(|tg| o.minutes >= tg.minutes) {
            let lim = |value: serde_json::Value, limit: serde_json::Value, pass: bool| Target { value, limit, pass };
            targets.insert("stand_max".into(), lim(time.stand.into(), tg.stand_max.into(), time.stand <= tg.stand_max));
            targets.insert("stand_move_max".into(), lim(time.stand_move.into(), tg.stand_move_max.into(), time.stand_move <= tg.stand_move_max));
            let distinct = behaviors.len();
            targets.insert("distinct_min".into(), lim(distinct.into(), tg.distinct_min.into(), distinct >= tg.distinct_min as usize));
            let mut per_group: BTreeMap<&str, u64> = BTreeMap::new();
            for g in &groups {
                *per_group.entry(g).or_default() += 1;
            }
            let top = per_group.values().copied().max().map_or(0.0, |n| r3(n as f64 / picks.max(1) as f64));
            targets.insert("share_max".into(), lim(top.into(), tg.share_max.into(), top <= tg.share_max));
            let lens: Vec<f64> = long.iter().map(|l| l.len_s).collect();
            let (lo, hi) = (lens.iter().copied().fold(f64::INFINITY, f64::min), lens.iter().copied().fold(0.0, f64::max));
            let ok = !lens.is_empty() && lens.iter().all(|l| *l >= tg.long_len_s[0] && *l <= tg.long_len_s[1]);
            let value = if lens.is_empty() { serde_json::json!([]) } else { serde_json::json!([lo, hi]) };
            targets.insert("long_len_s".into(), lim(value, serde_json::json!(tg.long_len_s), ok));
        }
    }
    let pass = pack.variety.is_none() || (violations.is_empty() && targets.values().all(|t| t.pass));
    Ok(Run {
        seed: o.seed,
        start: o.start.clone(),
        picks,
        distinct: behaviors.len(),
        relaxed: relaxed.len() as u64,
        sequence: seq.into_iter().map(|(_, n)| n).collect(),
        time,
        behaviors,
        kinds,
        long,
        violations,
        targets,
        pass,
    })
}

/// The v2 chart: the v1 bars and sequence strip of the first run, then a stacked time row
/// (stand, walk_run, turn, other).
pub fn chart_v2(r: &Run, order: &[String]) -> (usize, usize, Vec<u8>) {
    let mut plays: BTreeMap<String, u64> = BTreeMap::new();
    for b in &r.sequence {
        *plays.entry(b.clone()).or_default() += 1;
    }
    let a = Audit { pack: String::new(), minutes: 0.0, seed: r.seed, plays, starts: r.picks, distinct: r.distinct, max_same_in_row: 0, entropy: 0.0, sequence: r.sequence.clone() };
    let (w, h0, mut px) = chart(&a, order);
    let (row, pad) = (12usize, 10usize);
    let h = h0 + row + pad;
    px.resize(w * h * 4, 0);
    let cols = [[0xc2u8, 0xa0, 0x6e], [0x4f, 0x8a, 0x83], [0x8b, 0x78, 0xad], [0x9a, 0x8f, 0x7a]];
    let shares = [r.time.stand, r.time.walk_run, r.time.turn, r.time.other.max(0.0)];
    let (gap, span) = (12usize, w - 24);
    let mut x = gap;
    for y in h0..h {
        for xx in 0..w {
            let i = (y * w + xx) * 4;
            px[i..i + 4].copy_from_slice(&[0xf4, 0xf1, 0xea, 255]);
        }
    }
    for (k, sh) in shares.iter().enumerate() {
        let n = if k == shares.len() - 1 { gap + span - x } else { (sh * span as f64).round() as usize };
        for xx in x..(x + n).min(gap + span) {
            for y in h0..h0 + row {
                let i = (y * w + xx) * 4;
                px[i..i + 4].copy_from_slice(&[cols[k][0], cols[k][1], cols[k][2], 255]);
            }
        }
        x = (x + n).min(gap + span);
    }
    (w, h, px)
}

#[cfg(test)]
mod tests {
    #[test]
    fn times() {
        assert_eq!(super::start_minutes("10:30").unwrap(), 630);
        assert!(super::start_minutes("25:00").is_err());
        assert!(super::start_minutes("x").is_err());
        // 2026-11-16 is a Monday; 1970-01-01 a Thursday
        assert_eq!(super::parse_start("2026-11-16T10:00").unwrap(), (600, 0, 11));
        assert_eq!(super::parse_start("1970-01-01T00:00").unwrap(), (0, 3, 1));
        assert_eq!(super::parse_start("2024-02-29T23:59").unwrap(), (1439, 3, 2));
        assert!(super::parse_start("2026-13-01T10:00").is_err());
        assert!(super::parse_start("2026-02-31T10:00").is_err());
        assert!(super::parse_start("2026-02-29T10:00").is_err());
        assert!(super::parse_start("2026-04-31T10:00").is_err());
        assert!(super::parse_start("2026-11-16").is_err());
    }
}
