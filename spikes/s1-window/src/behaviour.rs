//! Demo-only behaviour loop: idle a while, then walk (or sometimes run) in a
//! random one of the 8 directions, chaining start -> loop xN -> stop through
//! the rest pose, then idle again. `S1_MODE=idle|walk|run` pins one gait for
//! measuring.

use crate::assets::{Library, DIRS};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Size {
    S100,
    S64,
}

pub struct Rng(u64);
impl Rng {
    pub fn seeded() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        Rng(t | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % (hi - lo + 1) as u64) as u32
    }
}

pub struct Behaviour {
    rng: Rng,
    queue: Vec<(String, u32)>, // (clip, loops) still to play, front = last
    pub clip: String,
    pub frame: usize,
    loops_left: u32,
    pub dir: &'static str,
    pub size: Size,
    mode: Option<String>,
}

impl Behaviour {
    pub fn new(size: Size) -> Self {
        let mode = std::env::var("S1_MODE").ok();
        let mut b = Behaviour {
            rng: Rng::seeded(),
            queue: vec![],
            clip: String::new(),
            frame: 0,
            loops_left: 0,
            dir: "S",
            size,
            mode,
        };
        b.reset(size);
        b
    }

    pub fn reset(&mut self, size: Size) {
        self.size = size;
        self.queue.clear();
        self.frame = 0;
        if size == Size::S64 {
            // only walk_64 S/SE exists; it is a size comparison, so just walk
            self.dir = if self.dir == "SE" { "SE" } else { "S" };
            self.clip = "walk_64".into();
            self.loops_left = 4;
        } else {
            self.clip = "idle".into();
            self.loops_left = 1;
        }
    }

    /// Advance one frame. Call after the current frame has been shown.
    pub fn advance(&mut self, lib: &Library) {
        let len = lib.clips[&self.clip].len();
        self.frame += 1;
        if self.frame < len {
            return;
        }
        self.frame = 0;
        if self.loops_left > 1 {
            self.loops_left -= 1;
            return;
        }
        if let Some((c, n)) = self.queue.pop() {
            self.clip = c;
            self.loops_left = n;
            return;
        }
        self.plan(lib);
    }

    /// End the current idle at its next frame boundary.
    pub fn force_leave_idle(&mut self) {
        self.loops_left = 1;
        self.queue.clear();
    }

    fn plan(&mut self, lib: &Library) {
        if self.size == Size::S64 {
            let c = &lib.clips["walk_64"];
            let dirs: Vec<_> = DIRS.iter().copied().filter(|d| c.has_dir(d)).collect();
            self.dir = dirs[self.rng.range(0, dirs.len() as u32 - 1) as usize];
            self.clip = "walk_64".into();
            self.loops_left = self.rng.range(2, 5);
            return;
        }
        let mode = self.mode.clone();
        match mode.as_deref() {
            Some("idle") => {
                self.clip = "idle".into();
                self.loops_left = 1;
                return;
            }
            Some("walk") | Some("run") => {
                // continuous gait loop, change direction every few loops
                let g = mode.unwrap();
                self.dir = DIRS[self.rng.range(0, 7) as usize];
                self.clip = g;
                self.loops_left = 4;
                return;
            }
            _ => {}
        }
        if self.clip == "idle" {
            // leave idle: pick a direction and gait, queue start -> loop -> stop
            self.dir = DIRS[self.rng.range(0, 7) as usize];
            let run = self.rng.range(0, 99) < 30;
            let (g, n) = if run { ("run", self.rng.range(2, 5)) } else { ("walk", self.rng.range(2, 4)) };
            self.clip = format!("{g}_start");
            self.loops_left = 1;
            // queue is popped from the back
            self.queue = vec![(String::from("idle"), self.rng.range(1, 3)), (format!("{g}_stop"), 1), (g.to_string(), n)];
        } else {
            self.clip = "idle".into();
            self.loops_left = self.rng.range(1, 2);
        }
    }
}
