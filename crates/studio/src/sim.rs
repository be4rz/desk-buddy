//! The studio's harness simulator: its own `Brain` on a virtual clock, fed the frozen
//! `MascotEvent` shapes (§4.1) and `SensesPatch`es / `Timeline`s (§4.4). It never talks to
//! the live agent or a model and needs no key.

use std::sync::Arc;

use mascot_core::replay::{start_minutes, Item, SensesPatch, Timeline};
use mascot_core::{Brain, BrainConfig, DndAction, LocalTime, PackMeta, Poke, Senses, Show, TraceLine};
use mascot_protocol::pack::Temperament;
use mascot_protocol::MascotEvent;

pub struct Sim {
    pack: Arc<PackMeta>,
    brain: Brain,
    senses: Senses,
    t: u64,
    seed: u64,
    temperament: Option<Temperament>,
    /// Timeline items not applied yet, in `at_ms` order (times relative to `t0`).
    queue: Vec<Item>,
    t0: u64,
}

impl Sim {
    pub fn new(pack: Arc<PackMeta>, seed: u64) -> Sim {
        let brain = Brain::new(pack.clone(), BrainConfig { seed, temperament: None, dnd_action: DndAction::Still }, 0);
        let senses = Senses { local: LocalTime { minutes: 600, weekday: 0 }, ..Senses::default() };
        let mut s = Sim { pack, brain, senses, t: 0, seed, temperament: None, queue: vec![], t0: 0 };
        s.brain.senses(&s.senses, 0);
        s
    }

    /// Start over (same pack), keeping the temperament.
    pub fn reset(&mut self, seed: u64) {
        let t = self.temperament.clone();
        *self = Sim::new(self.pack.clone(), seed);
        if let Some(t) = t {
            self.set_temperament(t);
        }
    }

    pub fn now(&self) -> u64 {
        self.t
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn set_temperament(&mut self, t: Temperament) {
        self.brain.set_temperament(t.clone());
        self.temperament = Some(t);
    }

    pub fn event(&mut self, ev: &MascotEvent) {
        self.brain.event(ev, self.t);
    }

    pub fn senses(&mut self, p: &SensesPatch) {
        p.apply(&mut self.senses);
        self.brain.senses(&self.senses, self.t);
    }

    pub fn poke(&mut self, p: Poke) {
        self.brain.poke(p, self.t);
    }

    /// Run a timeline from now: the brain restarts with its seed and its items play as the
    /// clock passes them. A bad `start_local` (not `HH:MM`) is refused, as `buddy studio audit`
    /// and the replay refuse it.
    pub fn timeline(&mut self, tl: Timeline) -> Result<(), String> {
        let minutes = start_minutes(&tl.start_local)?;
        self.reset(tl.seed);
        self.senses.local = LocalTime { minutes, weekday: tl.weekday.min(6) };
        self.brain.senses(&self.senses, 0);
        let mut items = tl.items;
        items.sort_by_key(|i| i.at_ms);
        self.queue = items;
        self.t0 = self.t;
        Ok(())
    }

    /// The pack reloaded: start over on it with the same seed, keeping the temperament and the
    /// senses the page set (a running timeline stops).
    pub fn reload(&mut self, pack: Arc<PackMeta>) {
        let (t, senses) = (self.temperament.clone(), self.senses.clone());
        *self = Sim::new(pack, self.seed);
        self.senses = senses;
        self.brain.senses(&self.senses, 0);
        if let Some(t) = t {
            self.set_temperament(t);
        }
    }

    /// Advance the clock by `dt_ms` (capped at 10 s per call), applying due timeline items on
    /// the way, and return what shows now.
    pub fn tick(&mut self, dt_ms: u64) -> Show {
        let end = self.t + dt_ms.min(10_000);
        loop {
            let next_item = self.queue.first().map_or(u64::MAX, |i| self.t0 + i.at_ms);
            if next_item > end {
                break;
            }
            self.t = next_item.max(self.t);
            let it = self.queue.remove(0);
            if let Some(p) = &it.senses {
                p.apply(&mut self.senses);
                self.brain.senses(&self.senses, self.t);
            }
            if let Some(e) = &it.event {
                self.brain.event(e, self.t);
            }
            if let Some(p) = it.poke {
                self.brain.poke(p, self.t);
            }
            self.brain.tick(self.t);
        }
        self.t = end;
        self.brain.tick(self.t)
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn trace(&self) -> &[TraceLine] {
        self.brain.trace()
    }
}
