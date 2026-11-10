//! Where credentials persist (design §3.1): the OS keychain, service `desk-buddy`,
//! account `provider:<id>`, holding pi-ai's `Credential` JSON. Tests use `MemoryStore`.
//! Nothing here ever logs or formats a secret: errors carry the store's reason only.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use serde_json::Value;

pub const SERVICE: &str = "desk-buddy";
/// A provider's keychain account is `provider:<id>`.
pub const PREFIX: &str = "provider:";

#[derive(Clone, Debug, PartialEq)]
pub struct StoreError {
    pub message: String,
}

impl StoreError {
    pub fn new(m: impl Into<String>) -> StoreError {
        StoreError { message: m.into() }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

pub type StoreRes<T> = Result<T, StoreError>;

pub trait CredentialStore: Send + Sync {
    /// The stored credential, `None` when there is none.
    fn get(&self, provider_id: &str) -> StoreRes<Option<Value>>;
    fn put(&self, provider_id: &str, credential: &Value) -> StoreRes<()>;
    /// Removing a missing item is not an error.
    fn delete(&self, provider_id: &str) -> StoreRes<()>;
    /// Provider ids with a stored item, sorted (no secret is read).
    fn list(&self) -> StoreRes<Vec<String>>;
}

/// In memory, for tests and `BUDDY_CRED_STORE=memory|fail` (debug builds).
#[derive(Default)]
pub struct MemoryStore {
    items: Mutex<BTreeMap<String, String>>,
    /// Every read fails, as a keychain that denies access.
    fail_reads: bool,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// A store whose reads fail with "access denied".
    pub fn failing() -> MemoryStore {
        MemoryStore { fail_reads: true, ..MemoryStore::default() }
    }

    fn items(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn check(&self) -> StoreRes<()> {
        if self.fail_reads {
            return Err(StoreError::new("access denied"));
        }
        Ok(())
    }
}

impl CredentialStore for MemoryStore {
    fn get(&self, provider_id: &str) -> StoreRes<Option<Value>> {
        self.check()?;
        self.items().get(provider_id).map(|s| serde_json::from_str(s).map_err(|_| StoreError::new("a stored item isn't a credential"))).transpose()
    }

    fn put(&self, provider_id: &str, credential: &Value) -> StoreRes<()> {
        self.items().insert(provider_id.into(), credential.to_string());
        Ok(())
    }

    fn delete(&self, provider_id: &str) -> StoreRes<()> {
        self.items().remove(provider_id);
        Ok(())
    }

    fn list(&self) -> StoreRes<Vec<String>> {
        self.check()?;
        Ok(self.items().keys().cloned().collect())
    }
}

/// The OS keychain (macOS login keychain; Windows Credential Manager).
pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new() -> KeychainStore {
        KeychainStore { service: SERVICE.into() }
    }

    /// Another service name (the keychain test uses its own).
    pub fn with_service(service: &str) -> KeychainStore {
        KeychainStore { service: service.into() }
    }

    fn entry(&self, provider_id: &str) -> StoreRes<keyring::Entry> {
        keyring::Entry::new(&self.service, &format!("{PREFIX}{provider_id}")).map_err(err)
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        KeychainStore::new()
    }
}

/// keyring's errors name the failure, never the secret.
fn err(e: keyring::Error) -> StoreError {
    StoreError::new(match e {
        keyring::Error::NoStorageAccess(inner) => format!("access denied ({inner})"),
        keyring::Error::PlatformFailure(inner) => format!("keychain error ({inner})"),
        keyring::Error::BadEncoding(_) => "a stored item isn't text".into(),
        other => other.to_string(),
    })
}

impl CredentialStore for KeychainStore {
    fn get(&self, provider_id: &str) -> StoreRes<Option<Value>> {
        match self.entry(provider_id)?.get_password() {
            Ok(s) => serde_json::from_str(&s).map(Some).map_err(|_| StoreError::new("a stored item isn't a credential")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    fn put(&self, provider_id: &str, credential: &Value) -> StoreRes<()> {
        self.entry(provider_id)?.set_password(&credential.to_string()).map_err(err)
    }

    fn delete(&self, provider_id: &str) -> StoreRes<()> {
        match self.entry(provider_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        }
    }

    #[cfg(target_os = "macos")]
    fn list(&self) -> StoreRes<Vec<String>> {
        use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
        // attributes only: listing never reads a secret, so it never asks for keychain access
        let found = ItemSearchOptions::new().class(ItemClass::generic_password()).service(&self.service).load_attributes(true).limit(Limit::All).search();
        let results = match found {
            Ok(r) => r,
            Err(e) if e.code() == -25300 => return Ok(vec![]), // errSecItemNotFound
            Err(e) => return Err(StoreError::new(format!("keychain error ({e})"))),
        };
        let mut ids: Vec<String> = results.iter().filter_map(|r| r.simplify_dict()).filter_map(|d| d.get("acct").and_then(|a| a.strip_prefix(PREFIX)).map(str::to_string)).collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// No enumeration elsewhere yet: the app checks the providers it knows with `get`.
    #[cfg(not(target_os = "macos"))]
    fn list(&self) -> StoreRes<Vec<String>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_store_round_trips_and_can_fail_reads() {
        let s = MemoryStore::new();
        assert_eq!(s.get("a").unwrap(), None);
        s.put("b", &json!({ "type": "api_key", "key": "k" })).unwrap();
        s.put("a", &json!({ "type": "oauth", "access": "x" })).unwrap();
        assert_eq!(s.list().unwrap(), vec!["a", "b"]);
        assert_eq!(s.get("b").unwrap().unwrap()["key"], "k");
        s.delete("b").unwrap();
        s.delete("b").unwrap();
        assert_eq!(s.list().unwrap(), vec!["a"]);
        let f = MemoryStore::failing();
        assert_eq!(f.list().unwrap_err().message, "access denied");
        assert!(f.get("a").is_err());
    }

    /// The real keychain, under its own service name. The human runs it at handover:
    /// `BUDDY_KEYCHAIN_TEST=1 cargo test -p bridge keychain`.
    #[test]
    fn keychain_round_trip() {
        if std::env::var("BUDDY_KEYCHAIN_TEST").ok().as_deref() != Some("1") {
            eprintln!("skipped: set BUDDY_KEYCHAIN_TEST=1 to use the real keychain");
            return;
        }
        let s = KeychainStore::with_service(&format!("desk-buddy-test-{}", std::process::id()));
        let cred = json!({ "type": "api_key", "key": format!("sk-test-{}", policy::random_hex(8)) });
        s.put("fake", &cred).unwrap();
        assert_eq!(s.get("fake").unwrap(), Some(cred.clone()));
        #[cfg(target_os = "macos")]
        assert_eq!(s.list().unwrap(), vec!["fake"]);
        s.delete("fake").unwrap();
        assert_eq!(s.get("fake").unwrap(), None);
        #[cfg(target_os = "macos")]
        assert!(s.list().unwrap().is_empty());
    }
}
