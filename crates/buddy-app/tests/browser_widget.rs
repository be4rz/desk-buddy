//! The browser widget's injected script on a real WKWebView (design `## widgets`): the app runs in
//! a temp `BUDDY_HOME`, the CLI drives `buddy widget open browser <fixture>` and `buddy browser
//! snapshot|click|type`. It opens real windows on the desktop, so it is `#[ignore]`d and run in QA:
//!
//!   cargo build -p host --bin buddy && cargo test -p buddy-app --test browser_widget -- --ignored
//!
//! The app is stopped with SIGTERM when the test ends, pass or fail.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct App {
    child: Child,
    home: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        // SAFETY: our own child's pid
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn buddy_bin() -> PathBuf {
    let b = Path::new(env!("CARGO_BIN_EXE_buddy-app")).with_file_name("buddy");
    assert!(b.is_file(), "build it first: cargo build -p host --bin buddy");
    b
}

/// Serve `tests/fixtures/page.html` (widgets crate) at `http://127.0.0.1:<port>/page.html`.
fn fixture_server() -> u16 {
    let page = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../widgets/tests/fixtures/page.html")).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s);
            let mut line = String::new();
            while r.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut s = r.into_inner();
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", page.len());
            let _ = s.write_all(&page);
        }
    });
    port
}

fn start() -> App {
    let home = std::env::temp_dir().join(format!("buddy-browser-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_buddy-app"))
        .env("BUDDY_HOME", &home)
        .env("BUDDY_SENSES", "0")
        .env("BUDDY_CRED_STORE", "memory")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let app = App { child, home };
    let t = Instant::now();
    while !app.home.join("host.json").is_file() {
        assert!(t.elapsed() < Duration::from_secs(20), "the app didn't start");
        std::thread::sleep(Duration::from_millis(100));
    }
    app
}

fn buddy(app: &App, args: &[&str]) -> Output {
    Command::new(buddy_bin()).env("BUDDY_HOME", &app.home).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
#[ignore = "opens real windows; run in QA"]
fn snapshot_click_and_type_on_a_real_page() {
    let port = fixture_server();
    let app = start();
    let url = format!("http://127.0.0.1:{port}/page.html");
    let o = buddy(&app, &["widget", "open", "browser", &url]);
    assert!(o.status.success(), "{}", text(&o));

    let snap = text(&buddy(&app, &["browser", "snapshot"]));
    assert!(snap.contains("# Order #4411"), "{snap}");
    assert!(snap.contains("[e1] link “Track package”"), "{snap}");
    assert!(snap.contains("[e2] field:text “Ask support”"), "{snap}");
    assert!(!snap.contains("hidden"), "hidden text stays out: {snap}");

    assert!(buddy(&app, &["browser", "click", "e1"]).status.success());
    assert!(text(&buddy(&app, &["browser", "snapshot"])).contains("tracked"));

    let o = buddy(&app, &["browser", "type", "e2", "hello", "there", "--enter"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&buddy(&app, &["browser", "snapshot"])).contains("sent:hello there"));

    // what was typed never comes back: a password, a textarea (review p5 S2); a submit's caption does
    assert!(buddy(&app, &["browser", "type", "e4", "hunter2-secret"]).status.success());
    assert!(buddy(&app, &["browser", "type", "e5", "my private note"]).status.success());
    let snap = text(&buddy(&app, &["browser", "snapshot"]));
    assert!(snap.contains("[e4] field:password “”"), "{snap}");
    assert!(snap.contains("[e5] field “”"), "{snap}");
    assert!(snap.contains("[e6] field:submit “Go”"), "{snap}");
    assert!(!snap.contains("hunter2") && !snap.contains("private note"), "a typed value leaked: {snap}");

    // non-happy: a stale ref
    let o = buddy(&app, &["browser", "click", "e999"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("no such element: e999"), "{}", text(&o));
}
