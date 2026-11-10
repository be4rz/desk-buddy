//! The model credentials: persisted in a `CredentialStore` (the OS keychain, design
//! §3.1) and cached in this process's memory. The sidecar reads them with `cred.*`
//! requests; a key never reaches its environment, a command's environment, a log or a
//! file. Session credentials (the fake model's key, a debug build's env key) live in
//! memory only and are never written to the store.

use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::store::{CredentialStore, MemoryStore, StoreError};

/// How long `cred.modify_begin` waits for another start's lease on the same provider.
const LEASE_WAIT: Duration = Duration::from_secs(30);
/// A lease nobody ended in this long is free again (a sidecar that lost track of it).
const LEASE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Inner {
    creds: HashMap<String, Value>,
    /// Providers whose credential is this session's only (never stored).
    session: HashSet<String>,
    /// Provider → the lease that is modifying it, and when it began.
    leases: HashMap<String, (String, Instant)>,
    /// Providers whose store copy may be older than memory: a write that outlived its lease
    /// landed after a newer one. The next lease to end there (or a restore) writes memory back.
    stale: HashSet<String>,
    next: u64,
}

type Guard<'a> = std::sync::MutexGuard<'a, Inner>;

pub struct Credentials {
    inner: Mutex<Inner>,
    freed: Condvar,
    store: Box<dyn CredentialStore>,
}

impl Default for Credentials {
    fn default() -> Self {
        Credentials::with_store(Box::new(MemoryStore::new()))
    }
}

type Fail = (String, String);

fn fail(code: &str, message: &str) -> Fail {
    (code.into(), message.into())
}

impl Credentials {
    /// Over a `MemoryStore` (tests, tools).
    pub fn new() -> Credentials {
        Credentials::default()
    }

    pub fn with_store(store: Box<dyn CredentialStore>) -> Credentials {
        Credentials { inner: Mutex::default(), freed: Condvar::new(), store }
    }

    /// Read every stored credential into memory (once, at start). An error means the
    /// store can't be read (a denied keychain): the app starts no agent.
    pub fn load(&self) -> Result<usize, StoreError> {
        let ids = self.store.list()?;
        let mut got = vec![];
        for id in ids {
            if let Some(c) = self.store.get(&id)? {
                if valid(&c) {
                    got.push((id, c));
                }
            }
        }
        let n = got.len();
        let mut g = self.lock();
        for (id, c) in got {
            if !g.session.contains(&id) {
                g.creds.insert(id, c);
            }
        }
        Ok(n)
    }

    /// A credential for this session only, never stored (the fake model, a debug env key).
    pub fn set_session(&self, provider_id: &str, credential: Value) {
        let mut g = self.lock();
        g.session.insert(provider_id.into());
        g.creds.insert(provider_id.into(), credential);
    }

    /// A session-only API key credential, pi-ai's shape.
    pub fn set_api_key(&self, provider_id: &str, key: &str) {
        self.set_session(provider_id, json!({ "type": "api_key", "key": key }));
    }

    /// Store a credential (the sign-in window's pasted key): the store first, then memory.
    pub fn put(&self, provider_id: &str, credential: Value) -> Result<(), StoreError> {
        if !valid(&credential) {
            return Err(StoreError::new("That isn’t a key or a sign-in."));
        }
        self.store.put(provider_id, &credential)?;
        let mut g = self.lock();
        g.session.remove(provider_id);
        g.creds.insert(provider_id.into(), credential);
        Ok(())
    }

    /// Sign out: the stored item goes, then the memory copy.
    pub fn remove(&self, provider_id: &str) -> Result<(), StoreError> {
        if !self.lock().session.contains(provider_id) {
            self.store.delete(provider_id)?;
        }
        let mut g = self.lock();
        g.session.remove(provider_id);
        g.creds.remove(provider_id);
        Ok(())
    }

    pub fn has(&self, provider_id: &str) -> bool {
        self.lock().creds.contains_key(provider_id)
    }

    /// The credential is this session's only (an env key, the fake model's).
    pub fn is_session(&self, provider_id: &str) -> bool {
        self.lock().session.contains(provider_id)
    }

    /// `(provider id, "api_key" | "oauth")`, sorted; never the secret.
    pub fn kinds(&self) -> Vec<(String, String)> {
        let g = self.lock();
        let mut v: Vec<(String, String)> = g.creds.iter().map(|(p, c)| (p.clone(), c["type"].as_str().unwrap_or("").to_string())).collect();
        v.sort();
        v
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One `cred.*` request.
    pub fn request(&self, op: &str, req: &Value) -> Result<Value, Fail> {
        let provider = req["providerId"].as_str().unwrap_or("");
        match op {
            "cred.read" => Ok(self.lock().creds.get(provider).cloned().unwrap_or(Value::Null)),
            "cred.list" => {
                let g = self.lock();
                let mut list: Vec<Value> = g.creds.iter().map(|(p, c)| json!({ "providerId": p, "type": c["type"] })).collect();
                list.sort_by(|a, b| a["providerId"].as_str().cmp(&b["providerId"].as_str()));
                Ok(Value::Array(list))
            }
            "cred.modify_begin" => {
                if provider.is_empty() {
                    return Err(fail("bad_request", "Name a provider."));
                }
                let until = Instant::now() + LEASE_WAIT;
                let mut g = self.lock();
                // Until it ends, or until it's too old to be a live refresh.
                while let Some(held) = g.leases.get(provider).map(|(_, at)| LEASE_TTL.saturating_sub(at.elapsed())).filter(|h| !h.is_zero()) {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(fail("busy", "Another sign-in refresh is still running."));
                    }
                    g = self.freed.wait_timeout(g, left.min(held)).unwrap_or_else(|e| e.into_inner()).0;
                }
                g.next += 1;
                let lease = format!("l{}-{provider}", g.next);
                g.leases.insert(provider.into(), (lease.clone(), Instant::now()));
                Ok(json!({ "lease": lease, "credential": g.creds.get(provider).cloned().unwrap_or(Value::Null) }))
            }
            "cred.modify_end" => {
                let lease = req["lease"].as_str().unwrap_or("");
                let mut g = self.lock();
                // An old lease still ends with its write unless another refresh took the provider.
                let Some(provider) = g.leases.iter().find(|(_, (l, _))| l.as_str() == lease).map(|(p, _)| p.clone()) else {
                    return Err(fail("not_found", "That lease has ended."));
                };
                let bad = req["write"] == true && !valid(&req["credential"]);
                let write = (req["write"] == true && !bad).then(|| req["credential"].clone());
                let mut stored = Ok(());
                if !g.session.contains(&provider) {
                    // this refresh's credential, or (owed) memory's, when the store may be behind
                    let put = write.clone().or_else(|| g.stale.contains(&provider).then(|| g.creds.get(&provider).cloned()).flatten());
                    if let Some(c) = put {
                        let (g2, r, ours) = self.write_through(g, &provider, lease, &c);
                        g = g2;
                        if !ours {
                            // a slow prompt outlived the lease and another refresh took the provider:
                            // theirs wins, in memory and, once they're done, in the store
                            self.restore(g, &provider);
                            return Err(fail("not_found", "That lease has ended."));
                        }
                        if write.is_some() {
                            stored = r;
                        } else if let Err(e) = r {
                            eprintln!("[bridge] can't bring {provider}'s stored sign-in up to date: {e}");
                        }
                    }
                }
                if let Some(c) = write {
                    // Kept in memory even when the store refused it: a refresh may have spent the old
                    // refresh token, so the old credential would be dead. The store error is still reported.
                    g.creds.insert(provider.clone(), c);
                }
                // The lease ends either way; a refresh writes a new key or sign-in, never removes one.
                g.leases.remove(&provider);
                let credential = g.creds.get(&provider).cloned().unwrap_or(Value::Null);
                drop(g);
                self.freed.notify_all();
                if bad {
                    return Err(fail("bad_request", "That isn’t a key or a sign-in."));
                }
                if let Err(e) = stored {
                    return Err(fail("store", &format!("Couldn’t save the sign-in: {e}")));
                }
                Ok(json!({ "credential": credential }))
            }
            // Keys and sign-ins are removed in the app, never by the agent.
            "cred.delete" => Err(fail("unsupported", "Keys and sign-ins are removed in desk-buddy’s settings.")),
            _ => Err(fail("unknown_op", op)),
        }
    }

    /// Store `c` for `provider` while `lease` holds it, unlocked (a keychain prompt can take a
    /// while). Returns the guard, the store's answer, and whether the lease is still ours. A late
    /// write of an ended lease that landed meanwhile (`stale`) means ours goes again.
    fn write_through<'a>(&'a self, mut g: Guard<'a>, provider: &str, lease: &str, c: &Value) -> (Guard<'a>, Result<(), StoreError>, bool) {
        loop {
            g.stale.remove(provider);
            drop(g);
            let r = self.store.put(provider, c);
            g = self.lock();
            if !g.leases.get(provider).is_some_and(|(l, _)| l == lease) {
                g.stale.insert(provider.into());
                return (g, r, false);
            }
            if r.is_err() {
                // the store keeps an older copy: the next lease to end tries again
                g.stale.insert(provider.into());
            }
            if r.is_err() || !g.stale.contains(provider) {
                return (g, r, true);
            }
        }
    }

    /// After a late write landed: put memory's credential back in the store, under a lease of
    /// its own. When another refresh holds the provider, its end does it (`stale` is set).
    fn restore<'a>(&'a self, mut g: Guard<'a>, provider: &str) {
        while !g.leases.contains_key(provider) && g.stale.contains(provider) {
            let Some(c) = g.creds.get(provider).cloned() else {
                g.stale.remove(provider);
                break;
            };
            g.next += 1;
            let lease = format!("l{}-{provider}-restore", g.next);
            g.leases.insert(provider.into(), (lease.clone(), Instant::now()));
            let (g2, r, ours) = self.write_through(g, provider, &lease, &c);
            g = g2;
            if ours {
                g.leases.remove(provider);
                if let Err(e) = r {
                    eprintln!("[bridge] can't restore {provider}'s newer sign-in in the store: {e}");
                }
                break;
            }
        }
        drop(g);
        self.freed.notify_all();
    }

    /// The sidecar went away mid-modify: free its leases.
    pub fn release_all(&self, leases: &[String]) {
        let mut g = self.lock();
        g.leases.retain(|_, (l, _)| !leases.contains(l));
        drop(g);
        self.freed.notify_all();
    }
}

/// pi-ai's credential shapes: an API key, or an OAuth sign-in.
fn valid(c: &Value) -> bool {
    let has = |k: &str| c[k].as_str().is_some_and(|v| !v.is_empty());
    match c["type"].as_str() {
        Some("api_key") => has("key") || c["env"].is_object(),
        Some("oauth") => has("access"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lists_and_modifies_under_a_lease() {
        let c = Credentials::new();
        c.set_api_key("fake", "sk-test");
        assert_eq!(c.request("cred.read", &json!({ "providerId": "fake" })).unwrap()["key"], "sk-test");
        assert_eq!(c.request("cred.read", &json!({ "providerId": "none" })).unwrap(), Value::Null);
        assert_eq!(c.request("cred.list", &json!({})).unwrap(), json!([{ "providerId": "fake", "type": "api_key" }]));
        let b = c.request("cred.modify_begin", &json!({ "providerId": "fake" })).unwrap();
        let lease = b["lease"].as_str().unwrap().to_string();
        let end = c.request("cred.modify_end", &json!({ "lease": lease, "write": true, "credential": { "type": "api_key", "key": "sk-new" } })).unwrap();
        assert_eq!(end["credential"]["key"], "sk-new");
        assert!(c.request("cred.modify_end", &json!({ "lease": lease })).is_err(), "a lease ends once");
        // A write that isn't a credential is refused and leaves the key.
        let b = c.request("cred.modify_begin", &json!({ "providerId": "fake" })).unwrap();
        let bad = c.request("cred.modify_end", &json!({ "lease": b["lease"], "write": true, "credential": null }));
        assert_eq!(bad.unwrap_err().0, "bad_request");
        assert!(c.request("cred.modify_end", &json!({ "lease": b["lease"] })).is_err(), "the refused write still ended the lease");
        assert_eq!(c.request("cred.delete", &json!({ "providerId": "fake" })).unwrap_err().0, "unsupported");
        assert_eq!(c.request("cred.read", &json!({ "providerId": "fake" })).unwrap()["key"], "sk-new");
    }

    /// A store that counts writes and can refuse them.
    struct Spy(Arc<MemoryStore>, Arc<std::sync::atomic::AtomicBool>);

    impl CredentialStore for Spy {
        fn get(&self, p: &str) -> crate::store::StoreRes<Option<Value>> {
            self.0.get(p)
        }
        fn put(&self, p: &str, c: &Value) -> crate::store::StoreRes<()> {
            if self.1.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(StoreError::new("keychain error (locked)"));
            }
            self.0.put(p, c)
        }
        fn delete(&self, p: &str) -> crate::store::StoreRes<()> {
            self.0.delete(p)
        }
        fn list(&self) -> crate::store::StoreRes<Vec<String>> {
            self.0.list()
        }
    }

    use std::sync::Arc;

    #[test]
    fn stored_credentials_load_writes_persist_and_session_keys_never_do() {
        let mem = Arc::new(MemoryStore::new());
        mem.put("anthropic", &json!({ "type": "oauth", "access": "a1", "refresh": "r", "expires": 1 })).unwrap();
        mem.put("junk", &json!({ "type": "nope" })).unwrap();
        let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c = Credentials::with_store(Box::new(Spy(mem.clone(), refuse.clone())));
        assert_eq!(c.load().unwrap(), 1, "only valid credentials load");
        assert!(c.has("anthropic") && !c.has("junk"));
        // the sidecar's refresh writes through to the store
        let b = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        c.request("cred.modify_end", &json!({ "lease": b["lease"], "write": true, "credential": { "type": "oauth", "access": "a2" } })).unwrap();
        assert_eq!(mem.get("anthropic").unwrap().unwrap()["access"], "a2");
        // a session key is memory only, also when the sidecar writes it
        c.set_api_key("fake", "sk-session");
        let b = c.request("cred.modify_begin", &json!({ "providerId": "fake" })).unwrap();
        c.request("cred.modify_end", &json!({ "lease": b["lease"], "write": true, "credential": { "type": "api_key", "key": "sk-2" } })).unwrap();
        assert_eq!(mem.get("fake").unwrap(), None);
        assert_eq!(c.request("cred.read", &json!({ "providerId": "fake" })).unwrap()["key"], "sk-2");
        // a refused store write says so, without the secret, and keeps the refreshed credential in
        // memory for this run (the refresh may have spent the old refresh token); the store keeps the old
        refuse.store(true, std::sync::atomic::Ordering::SeqCst);
        let b = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        let e = c.request("cred.modify_end", &json!({ "lease": b["lease"], "write": true, "credential": { "type": "oauth", "access": "SECRET-a3" } })).unwrap_err();
        assert_eq!(e.0, "store");
        assert!(!e.1.contains("SECRET"), "{}", e.1);
        assert_eq!(c.request("cred.read", &json!({ "providerId": "anthropic" })).unwrap()["access"], "SECRET-a3");
        assert_eq!(mem.get("anthropic").unwrap().unwrap()["access"], "a2");
        assert!(c.put("openrouter", json!({ "type": "api_key", "key": "k" })).is_err(), "the window's save fails too");
        assert!(!c.has("openrouter"));
        refuse.store(false, std::sync::atomic::Ordering::SeqCst);
        // the window's key and sign-out
        c.put("openrouter", json!({ "type": "api_key", "key": "sk-or" })).unwrap();
        assert!(c.put("x", json!({ "type": "api_key" })).is_err(), "not a credential");
        assert_eq!(c.kinds(), vec![("anthropic".into(), "oauth".into()), ("fake".into(), "api_key".into()), ("openrouter".into(), "api_key".into())]);
        c.remove("openrouter").unwrap();
        assert_eq!(mem.get("openrouter").unwrap(), None);
        assert!(!c.has("openrouter"));
    }

    /// A store whose first write waits until told (a keychain prompt nobody has answered yet).
    type Gate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
    struct Slow(Arc<MemoryStore>, std::sync::Mutex<Option<Gate>>);

    impl CredentialStore for Slow {
        fn get(&self, p: &str) -> crate::store::StoreRes<Option<Value>> {
            self.0.get(p)
        }
        fn put(&self, p: &str, c: &Value) -> crate::store::StoreRes<()> {
            let wait = self.1.lock().unwrap().take();
            if let Some((entered, go)) = wait {
                let _ = entered.send(());
                let _ = go.recv();
            }
            self.0.put(p, c)
        }
        fn delete(&self, p: &str) -> crate::store::StoreRes<()> {
            self.0.delete(p)
        }
        fn list(&self) -> crate::store::StoreRes<Vec<String>> {
            self.0.list()
        }
    }

    #[test]
    fn a_write_that_outlived_its_lease_leaves_the_newer_sign_in_in_memory_and_the_store() {
        let mem = Arc::new(MemoryStore::new());
        mem.put("anthropic", &json!({ "type": "oauth", "access": "a1" })).unwrap();
        let (go, go_rx) = std::sync::mpsc::channel();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let c = Arc::new(Credentials::with_store(Box::new(Slow(mem.clone(), std::sync::Mutex::new(Some((entered_tx, go_rx)))))));
        c.load().unwrap();
        let a = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        let la = a["lease"].as_str().unwrap().to_string();
        let ca = c.clone();
        let la2 = la.clone();
        let slow = std::thread::spawn(move || ca.request("cred.modify_end", &json!({ "lease": la2, "write": true, "credential": { "type": "oauth", "access": "a-old" } })));
        // A's write is stuck in the store; its lease goes (as when the sidecar restarts) and B refreshes
        entered.recv().unwrap();
        c.release_all(&[la]);
        let b = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        c.request("cred.modify_end", &json!({ "lease": b["lease"], "write": true, "credential": { "type": "oauth", "access": "a-new" } })).unwrap();
        go.send(()).unwrap();
        assert_eq!(slow.join().unwrap().unwrap_err().0, "not_found", "the late write's lease had ended");
        assert_eq!(c.request("cred.read", &json!({ "providerId": "anthropic" })).unwrap()["access"], "a-new");
        assert_eq!(mem.get("anthropic").unwrap().unwrap()["access"], "a-new", "the late write doesn't stay in the store");
    }

    #[test]
    fn a_late_write_while_another_refresh_holds_the_provider_is_undone_when_that_one_ends() {
        let mem = Arc::new(MemoryStore::new());
        mem.put("anthropic", &json!({ "type": "oauth", "access": "a1" })).unwrap();
        let (go, go_rx) = std::sync::mpsc::channel();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let c = Arc::new(Credentials::with_store(Box::new(Slow(mem.clone(), std::sync::Mutex::new(Some((entered_tx, go_rx)))))));
        c.load().unwrap();
        let a = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        let la = a["lease"].as_str().unwrap().to_string();
        let (ca, la2) = (c.clone(), la.clone());
        let slow = std::thread::spawn(move || ca.request("cred.modify_end", &json!({ "lease": la2, "write": true, "credential": { "type": "oauth", "access": "a-old" } })));
        entered.recv().unwrap();
        c.release_all(&[la]);
        // B holds the provider when A's write lands, and ends without writing
        let b = c.request("cred.modify_begin", &json!({ "providerId": "anthropic" })).unwrap();
        go.send(()).unwrap();
        assert_eq!(slow.join().unwrap().unwrap_err().0, "not_found");
        assert_eq!(mem.get("anthropic").unwrap().unwrap()["access"], "a-old", "(the late write landed)");
        c.request("cred.modify_end", &json!({ "lease": b["lease"] })).unwrap();
        assert_eq!(c.request("cred.read", &json!({ "providerId": "anthropic" })).unwrap()["access"], "a1");
        assert_eq!(mem.get("anthropic").unwrap().unwrap()["access"], "a1", "B's end put memory's copy back");
    }

    #[test]
    fn a_store_that_cant_be_read_fails_the_load() {
        let c = Credentials::with_store(Box::new(MemoryStore::failing()));
        assert_eq!(c.load().unwrap_err().message, "access denied");
        assert!(c.kinds().is_empty());
    }
}
