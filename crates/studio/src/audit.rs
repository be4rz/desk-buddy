//! `audit`: run the brain on a quiet timeline (or a given one) for N minutes, the same loop
//! as `mascot_core::replay::run`, and count which behaviors start. Seeded, so reproducible.

use std::collections::BTreeMap;
use std::sync::Arc;

use mascot_core::replay::{start_minutes, Timeline};
use mascot_core::{Brain, BrainConfig, DndAction, LocalTime, PackMeta, Senses};
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

#[cfg(test)]
mod tests {
    #[test]
    fn times() {
        assert_eq!(super::start_minutes("10:30").unwrap(), 630);
        assert!(super::start_minutes("25:00").is_err());
        assert!(super::start_minutes("x").is_err());
    }
}
