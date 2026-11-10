//! One desk-buddy per `BUDDY_HOME`. Before Tauri starts, the app takes an exclusive `flock` on
//! `BUDDY_HOME/app.lock` and holds it for its whole life (the kernel drops it at exit, crash
//! included, so there is never a stale lock). When another app holds it, this launch logs
//! `[buddy] already running (pid N)` (the pid from that app's `host.json`, when it's there yet)
//! and exits 0. Two launches at once can't both win: the lock is atomic.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;

/// Another app holds the lock: its pid, when its `host.json` says.
#[derive(Debug, PartialEq)]
pub struct Running(pub Option<u32>);

/// Take `home`'s app lock; keep the returned file open for the process's life (`None`: the
/// lock file can't be made, e.g. a read-only home; the launch isn't blocked over it).
pub fn claim(home: &Path) -> Result<Option<File>, Running> {
    let _ = std::fs::create_dir_all(home);
    let Ok(f) = OpenOptions::new().create(true).truncate(false).write(true).open(home.join("app.lock")) else {
        // can't even make the file (read-only home): don't block the launch over it
        eprintln!("[buddy] can't open {}: running without the one-app check", home.join("app.lock").display());
        return Ok(None);
    };
    // SAFETY: flock on a valid, open descriptor
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(f));
    }
    Err(Running(pid_of(&home.join("host.json"))))
}

fn pid_of(host_file: &Path) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(host_file).ok()?).ok()?;
    u32::try_from(v["pid"].as_u64()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_claim_wins_until_it_is_dropped() {
        let d = std::env::temp_dir().join(format!("buddy-instance-{}-{}", std::process::id(), host::random_hex(3)));
        let first = claim(&d).expect("first");
        // flock is per open file: a second open in this process stands in for a second app
        assert_eq!(claim(&d).unwrap_err(), Running(None), "no host.json yet");
        std::fs::write(d.join("host.json"), r#"{"port":1,"token":"t","pid":4242}"#).unwrap();
        assert_eq!(claim(&d).unwrap_err(), Running(Some(4242)));
        // the app quit (or crashed): the kernel dropped its lock
        drop(first);
        let again = claim(&d).expect("free again");
        drop(again);
        let _ = std::fs::remove_dir_all(&d);
    }
}
