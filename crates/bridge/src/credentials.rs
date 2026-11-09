//! The model credentials, in this process's memory only (the OS keychain is
//! S2c). The sidecar reads them with `cred.*` requests; a key never reaches its
//! environment, a command's environment or a file.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// How long `cred.modify_begin` waits for another start's lease on the same provider.
const LEASE_WAIT: Duration = Duration::from_secs(30);
/// A lease nobody ended in this long is free again (a sidecar that lost track of it).
const LEASE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Inner {
    creds: HashMap<String, Value>,
    /// Provider → the lease that is modifying it, and when it began.
    leases: HashMap<String, (String, Instant)>,
    next: u64,
}

#[derive(Default)]
pub struct Credentials {
    inner: Mutex<Inner>,
    freed: Condvar,
}

type Fail = (String, String);

fn fail(code: &str, message: &str) -> Fail {
    (code.into(), message.into())
}

impl Credentials {
    pub fn new() -> Credentials {
        Credentials::default()
    }

    pub fn set(&self, provider_id: &str, credential: Value) {
        self.lock().creds.insert(provider_id.into(), credential);
    }

    /// An API key credential, pi-ai's shape.
    pub fn set_api_key(&self, provider_id: &str, key: &str) {
        self.set(provider_id, json!({ "type": "api_key", "key": key }));
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
                // The lease ends either way; a refresh writes a new key or sign-in, never removes one.
                g.leases.remove(&provider);
                let bad = req["write"] == true && !valid(&req["credential"]);
                if req["write"] == true && !bad {
                    g.creds.insert(provider.clone(), req["credential"].clone());
                }
                let credential = g.creds.get(&provider).cloned().unwrap_or(Value::Null);
                drop(g);
                self.freed.notify_all();
                if bad {
                    return Err(fail("bad_request", "That isn’t a key or a sign-in."));
                }
                Ok(json!({ "credential": credential }))
            }
            // Keys and sign-ins are removed in the app, never by the agent.
            "cred.delete" => Err(fail("unsupported", "Keys and sign-ins are removed in desk-buddy’s settings.")),
            _ => Err(fail("unknown_op", op)),
        }
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
}
