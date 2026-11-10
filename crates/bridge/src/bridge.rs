// One sidecar process of a desk-buddy agent (ported from 2market): writes `init` and
// `cmd` records, reads its records on one thread (LF-only), routes `ack`s to
// the waiting command, runs `req`s on a small worker pool, and stops it with
// `shutdown`, then a kill after the grace period.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::protocol::{line, Records};
use crate::{BridgeError as RtError, Res};

/// Request threads per sidecar, for the quick requests.
const WORKERS: usize = 4;
/// Requests that can take minutes (an approval wait) or block on another's lease
/// (a sign-in refresh) run on a thread of their own, so they never hold up
/// credential reads or policy checks.
const LONG: [&str; 2] = ["policy.wait", "cred.modify_begin"];

/// What a sidecar's records reach.
pub trait Handler: Send + Sync + 'static {
    /// `ev`, `ready`, `fatal` and anything else that isn't an ack or a request.
    fn record(&self, gen: u64, rec: Value);
    /// One `req`, on a worker thread. `cancelled` turns true on its `cancel`.
    fn request(&self, gen: u64, op: &str, req: &Value, cancelled: &AtomicBool) -> Result<Value, (String, String)>;
    /// The sidecar stopped waiting for a request: an abort, or `ended` when
    /// the process is gone. Frees its waiter.
    fn cancelled(&self, gen: u64, op: &str, req: &Value, ended: bool);
    /// The process ended; `by_app` when Rust stopped it.
    fn exited(&self, gen: u64, code: Option<i32>, by_app: bool);
}

pub struct Spawn {
    pub program: PathBuf,
    pub cwd: PathBuf,
    /// The whole environment: the process starts from an empty one.
    pub env: Vec<(String, String)>,
    pub init: Value,
    pub gen: u64,
}

type Job = (String, String, Value, Arc<AtomicBool>);

pub struct Bridge {
    pub gen: u64,
    /// Also its process group (it leads its own).
    pub pid: u32,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    next: AtomicU64,
    waiting: Mutex<HashMap<String, mpsc::Sender<Value>>>,
    /// Requests in flight: their cancel flag, op and arguments.
    cancels: Mutex<HashMap<String, (Arc<AtomicBool>, String, Value)>>,
    running: AtomicBool,
    by_app: AtomicBool,
}

impl Bridge {
    pub fn spawn(spec: Spawn, handler: Arc<dyn Handler>) -> io::Result<Arc<Bridge>> {
        let mut cmd = Command::new(&spec.program);
        cmd.current_dir(&spec.cwd).env_clear().envs(spec.env.iter().map(|(k, v)| (k, v))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own group: a signal to the app's terminal doesn't reach it.
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| io::Error::other("no stderr"))?;
        let stdin = child.stdin.take();
        let b = Arc::new(Bridge {
            gen: spec.gen,
            pid: child.id(),
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            next: AtomicU64::new(0),
            waiting: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            running: AtomicBool::new(true),
            by_app: AtomicBool::new(false),
        });
        b.write(&spec.init)?;

        let (jobs, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let (rx, b, h) = (rx.clone(), b.clone(), handler.clone());
            thread::spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                let Ok(job) = job else { return };
                b.run(&*h, job);
            });
        }

        thread::spawn(move || {
            for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("buddy-agent[{}]: {l}", spec.gen);
            }
        });

        let reader = b.clone();
        thread::spawn(move || {
            for rec in Records::new(BufReader::with_capacity(256 * 1024, stdout)) {
                let id = rec["id"].as_str().unwrap_or("").to_string();
                match rec["t"].as_str() {
                    Some("ack") => {
                        if let Some(w) = reader.waiting.lock().unwrap().remove(&id) {
                            let _ = w.send(rec);
                        }
                    }
                    Some("req") => {
                        let flag = Arc::new(AtomicBool::new(false));
                        let op = rec["op"].as_str().unwrap_or("").to_string();
                        reader.cancels.lock().unwrap().insert(id.clone(), (flag.clone(), op.clone(), rec.clone()));
                        if LONG.contains(&op.as_str()) {
                            let (b, h) = (reader.clone(), handler.clone());
                            thread::spawn(move || b.run(&*h, (id, op, rec, flag)));
                        } else {
                            let _ = jobs.send((id, op, rec, flag));
                        }
                    }
                    Some("cancel") => {
                        let found = reader.cancels.lock().unwrap().remove(&id);
                        if let Some((f, op, req)) = found {
                            f.store(true, Ordering::SeqCst);
                            handler.cancelled(reader.gen, &op, &req, false);
                        }
                    }
                    _ => handler.record(reader.gen, rec),
                }
            }
            drop(jobs);
            // Every pending request and command ends with the process.
            let left: Vec<_> = reader.cancels.lock().unwrap().drain().map(|(_, v)| v).collect();
            for (f, op, req) in left {
                f.store(true, Ordering::SeqCst);
                handler.cancelled(reader.gen, &op, &req, true);
            }
            reader.waiting.lock().unwrap().clear();
            *reader.stdin.lock().unwrap() = None;
            let code = reader.child.lock().unwrap().wait().ok().and_then(|s| s.code());
            // Exit handled before `running` drops: a restart waiting in
            // `shutdown` must not start the next gen first.
            handler.exited(reader.gen, code, reader.by_app.load(Ordering::SeqCst));
            reader.running.store(false, Ordering::SeqCst);
        });
        Ok(b)
    }

    /// One request, answered unless it was cancelled.
    fn run(&self, h: &dyn Handler, (id, op, req, cancelled): Job) {
        // Cancelled while queued: never run it.
        if cancelled.load(Ordering::SeqCst) {
            self.cancels.lock().unwrap().remove(&id);
            return;
        }
        let out = h.request(self.gen, &op, &req, &cancelled);
        self.cancels.lock().unwrap().remove(&id);
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        let res = match out {
            Ok(result) => json!({ "t": "res", "id": id, "ok": true, "result": result }),
            Err((code, message)) => json!({ "t": "res", "id": id, "ok": false, "error": { "code": code, "message": message } }),
        };
        let _ = self.write(&res);
    }

    fn write(&self, rec: &Value) -> io::Result<()> {
        let mut s = self.stdin.lock().unwrap();
        let w = s.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "the sidecar has stopped"))?;
        w.write_all(&line(rec))?;
        w.flush()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn next_id(&self) -> String {
        format!("c-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Send a command and wait for its ack's `result`.
    pub fn cmd(&self, op: &str, args: Value, wait: Duration) -> Res<Value> {
        let id = self.next_id();
        let mut rec = json!({ "t": "cmd", "id": id, "op": op });
        if let (Some(r), Value::Object(a)) = (rec.as_object_mut(), args) {
            for (k, v) in a {
                if !matches!(k.as_str(), "t" | "id" | "op") {
                    r.insert(k, v);
                }
            }
        }
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id.clone(), tx);
        if self.write(&rec).is_err() {
            self.waiting.lock().unwrap().remove(&id);
            return Err(gone());
        }
        match rx.recv_timeout(wait) {
            Ok(ack) if ack["ok"] == true => Ok(ack["result"].clone()),
            Ok(ack) => Err(RtError { code: code_of(ack["error"]["code"].as_str().unwrap_or("error")), message: ack["error"]["message"].as_str().unwrap_or("The agent refused that.").to_string() }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(gone()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.waiting.lock().unwrap().remove(&id);
                Err(RtError::new("timeout", "The agent didn’t answer in time."))
            }
        }
    }

    /// Ask it to stop; kill it after `grace`. Returns once it has ended.
    pub fn shutdown(&self, grace: Duration) {
        self.by_app.store(true, Ordering::SeqCst);
        if self.is_running() {
            let id = self.next_id();
            let _ = self.write(&json!({ "t": "cmd", "id": id, "op": "shutdown" }));
        }
        let until = Instant::now() + grace;
        while self.is_running() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        if self.is_running() {
            self.kill();
            let until = Instant::now() + Duration::from_secs(2);
            while self.is_running() && Instant::now() < until {
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }

    /// SIGKILL its process group, as a crash would end it (tests).
    #[cfg(unix)]
    pub fn kill_group(&self) {
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", self.pid)]).status();
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.child.lock().unwrap().kill();
        }
    }
}

fn gone() -> RtError {
    RtError::new("not_running", "The agent has stopped.")
}

/// The sidecar's error codes the webview acts on; anything else is an error.
fn code_of(code: &str) -> &'static str {
    match code {
        "no_provider" => "no_provider",
        "bad_request" => "usage",
        "not_found" => "not_found",
        "unknown_op" => "unknown_op",
        "unsupported" => "unsupported",
        "too_many" => "too_many",
        // Shutting down: the command is for the next sidecar.
        "closing" => "not_running",
        _ => "agent_error",
    }
}

/// The sidecar: `buddy-agent` beside the app; debug builds first take
/// `BUDDY_AGENT_BIN` when set and not empty (no other is tried, so a wrong path
/// isn't hidden), and last the repo's `agent/dist/buddy-agent`. A release build
/// never runs a binary its environment names: that binary would get the keys.
pub fn sidecar_bin() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("BUDDY_AGENT_BIN").filter(|v| !v.is_empty()).map(PathBuf::from) {
            return Some(p).filter(|p| p.is_file());
        }
    }
    let name = if cfg!(windows) { "buddy-agent.exe" } else { "buddy-agent" };
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).filter(|p| p.is_file());
    beside.or_else(|| cfg!(debug_assertions).then(dev_bin).flatten())
}

/// The repo's build (`cd agent && bun run build`).
pub fn dev_bin() -> Option<PathBuf> {
    Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/dist/buddy-agent")).filter(|p| p.is_file())
}
