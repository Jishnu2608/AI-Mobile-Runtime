//! Authentication and authorisation for the local API (CORE-004).
//!
//! Two kinds of credentials:
//!
//! - **Admin token** — created fresh at every Core start and written to `run/admin.token`
//!   (owner-only file). First-party clients (the `umr` CLI, the desktop app) read it and get
//!   all scopes. It is deleted when the Core stops.
//! - **Client tokens** — created on request (`auth.client.create`, admin only) for other
//!   clients such as AI agents, with a limited set of scopes. Only a SHA-256 hash is stored
//!   in `clients.json`; the token itself is shown once to the creator.
//!
//! Scopes (from the spec): `observe` < `interact` < `control` < `admin`. A higher scope does
//! **not** imply the lower ones automatically except for `admin`, which grants everything.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What a client may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Read status, logs, diagnostics, screenshots.
    Observe,
    /// Send input to devices (tap, type, swipe).
    Interact,
    /// Run, reload, restart, rebuild, select devices.
    Control,
    /// Manage the Core itself: shutdown, clients, installs.
    Admin,
}

impl Scope {
    pub const ALL: [Scope; 4] = [
        Scope::Observe,
        Scope::Interact,
        Scope::Control,
        Scope::Admin,
    ];

    /// Parses `observe|interact|control|admin`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "observe" => Some(Self::Observe),
            "interact" => Some(Self::Interact),
            "control" => Some(Self::Control),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

/// An authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
}

impl Principal {
    /// Whether this principal may use `scope`.
    #[must_use]
    pub fn allows(&self, scope: Scope) -> bool {
        self.scopes.contains(&Scope::Admin) || self.scopes.contains(&scope)
    }
}

/// Prefix that makes tokens easy to recognise (and to scrub from logs later).
pub const TOKEN_PREFIX: &str = "umr_";

/// Creates a new random token: `umr_` + 64 hex chars (256 bits from the OS RNG).
pub fn generate_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(format!("{TOKEN_PREFIX}{}", hex(&bytes)))
}

/// SHA-256 of a token, hex encoded.
#[must_use]
pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Comparison that takes the same time whether or not the strings match early.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A registered (non-admin) client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRecord {
    pub id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub token_sha256: String,
    pub created_unix: u64,
    /// MCP-003: what this client may do without asking (`project:<folder>`, `device:switch`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<String>,
}

/// Public view of a client (never includes the hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSummary {
    pub id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub created_unix: u64,
    #[serde(default)]
    pub grants: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    clients: Vec<ClientRecord>,
}

/// Admin token + client registry.
#[derive(Debug)]
pub struct AuthStore {
    admin_token_sha256: String,
    registry_path: PathBuf,
    clients: Vec<ClientRecord>,
}

impl AuthStore {
    /// Loads the registry (empty if missing) and remembers the admin token's hash.
    pub fn load(registry_path: &Path, admin_token: &str) -> std::io::Result<Self> {
        let clients = match std::fs::read(registry_path) {
            Ok(bytes) => {
                let f: RegistryFile = serde_json::from_slice(&bytes).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{} is corrupt: {e}", registry_path.display()),
                    )
                })?;
                f.clients
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            admin_token_sha256: hash_token(admin_token),
            registry_path: registry_path.to_path_buf(),
            clients,
        })
    }

    /// Checks a presented token.
    #[must_use]
    pub fn verify(&self, token: &str) -> Option<Principal> {
        let h = hash_token(token);
        if constant_time_eq(&h, &self.admin_token_sha256) {
            return Some(Principal {
                client_id: "local-admin".into(),
                name: "local admin (CLI / desktop)".into(),
                scopes: Scope::ALL.to_vec(),
            });
        }
        self.clients
            .iter()
            .find(|c| constant_time_eq(&h, &c.token_sha256))
            .map(|c| Principal {
                client_id: c.id.clone(),
                name: c.name.clone(),
                scopes: c.scopes.clone(),
            })
    }

    /// Registers a client and returns its id and token (the token is not stored).
    pub fn create_client(
        &mut self,
        name: &str,
        scopes: &[Scope],
    ) -> std::io::Result<(String, String)> {
        let token = generate_token()?;
        let id = format!("c_{}", &hash_token(&token)[..12]);
        let mut scopes = scopes.to_vec();
        scopes.sort();
        scopes.dedup();
        self.clients.push(ClientRecord {
            id: id.clone(),
            name: name.to_string(),
            scopes,
            token_sha256: hash_token(&token),
            created_unix: now_unix(),
            grants: Vec::new(),
        });
        self.save()?;
        Ok((id, token))
    }

    /// Removes a client; returns whether it existed.
    pub fn revoke_client(&mut self, id: &str) -> std::io::Result<bool> {
        let before = self.clients.len();
        self.clients.retain(|c| c.id != id);
        let removed = self.clients.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    #[must_use]
    pub fn list_clients(&self) -> Vec<ClientSummary> {
        self.clients
            .iter()
            .map(|c| ClientSummary {
                id: c.id.clone(),
                name: c.name.clone(),
                scopes: c.scopes.clone(),
                created_unix: c.created_unix,
                grants: c.grants.clone(),
            })
            .collect()
    }

    /// A client's grants (empty for unknown clients and the admin).
    #[must_use]
    pub fn grants(&self, id: &str) -> Vec<String> {
        self.clients
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.grants.clone())
            .unwrap_or_default()
    }

    /// Replaces a client's grants; `false` if there is no such client.
    pub fn set_grants(&mut self, id: &str, grants: Vec<String>) -> std::io::Result<bool> {
        let Some(c) = self.clients.iter_mut().find(|c| c.id == id) else {
            return Ok(false);
        };
        c.grants = grants;
        c.grants.sort();
        c.grants.dedup();
        self.save()?;
        Ok(true)
    }

    /// Adds one grant.
    pub fn add_grant(&mut self, id: &str, grant: &str) -> std::io::Result<bool> {
        let mut g = self.grants(id);
        g.push(grant.to_string());
        self.set_grants(id, g)
    }

    fn save(&self) -> std::io::Result<()> {
        let f = RegistryFile {
            version: 1,
            clients: self.clients.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&f).map_err(std::io::Error::other)?;
        write_secret(&self.registry_path, &bytes)
    }
}

/// Writes a secret file readable only by the current user.
pub fn write_secret(path: &Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        umr_platform::secure_file::write_private(path, data)
    }
    #[cfg(not(windows))]
    {
        std::fs::write(path, data)
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn tokens_are_prefixed_random_and_long() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_eq!(a.len(), TOKEN_PREFIX.len() + 64);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_sha256_hex() {
        // SHA-256("abc")
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn admin_token_grants_everything_wrong_token_nothing() {
        let tmp = TempRoot::new("auth-admin");
        let store = AuthStore::load(&tmp.path().join("clients.json"), "umr_admin").unwrap();
        let p = store.verify("umr_admin").unwrap();
        assert!(Scope::ALL.iter().all(|s| p.allows(*s)));
        assert!(store.verify("umr_wrong").is_none());
        assert!(store.verify("").is_none());
    }

    #[test]
    fn client_lifecycle_persists_hashes_only() {
        let tmp = TempRoot::new("auth-clients");
        let path = tmp.path().join("clients.json");
        let mut store = AuthStore::load(&path, "umr_admin").unwrap();
        let (id, token) = store
            .create_client("claude-code", &[Scope::Observe, Scope::Interact])
            .unwrap();

        let p = store.verify(&token).unwrap();
        assert_eq!(p.client_id, id);
        assert!(p.allows(Scope::Observe) && p.allows(Scope::Interact));
        assert!(!p.allows(Scope::Control) && !p.allows(Scope::Admin));

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains(&token),
            "token must never be stored in clear text"
        );
        assert!(on_disk.contains(&hash_token(&token)));

        // reload from disk
        let store2 = AuthStore::load(&path, "umr_other_admin").unwrap();
        assert_eq!(store2.verify(&token).unwrap().client_id, id);
        assert_eq!(store2.list_clients().len(), 1);

        let mut store2 = store2;
        assert!(store2.revoke_client(&id).unwrap());
        assert!(store2.verify(&token).is_none());
        assert!(!store2.revoke_client(&id).unwrap());
    }

    #[test]
    fn corrupt_registry_is_reported() {
        let tmp = TempRoot::new("auth-corrupt");
        let path = tmp.path().join("clients.json");
        std::fs::write(&path, b"{oops").unwrap();
        assert!(AuthStore::load(&path, "umr_admin").is_err());
    }

    #[test]
    fn scope_parsing() {
        assert_eq!(Scope::parse("control"), Some(Scope::Control));
        assert_eq!(Scope::parse("root"), None);
    }
}
