//! RES-001: the Knowledge Pack — versioned, signed compatibility rules.
//!
//! The pack is JSON (`knowledge-pack/pack.json`) plus a detached Ed25519 signature
//! (`pack.json.sig`, base64). Every rule names a source (official URL + the date it was checked).
//! The pack is shipped inside the binary and can later be replaced by a newer signed pack
//! without a product update. A pack is accepted only if:
//! 1. its signature verifies against one of the [`TRUSTED_KEYS`], and
//! 2. it passes [`Pack::validate`] (known schema, parseable versions, referenced sources exist,
//!    no duplicates).

use std::collections::{BTreeMap, HashSet};

use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::version::Version;

/// Schema version this build understands.
pub const SCHEMA: u32 = 1;

/// Public keys whose signatures are accepted (base64, 32 bytes each).
///
/// `dev-2026-11` is the development key; its private half lives outside the repository
/// (`%USERPROFILE%\.umr\keys\knowledge-pack-dev.key`). A release key must replace it before
/// public release (decision pending: key custody).
pub const TRUSTED_KEYS: &[(&str, &str)] = &[(
    "dev-2026-11",
    include_str!("../../../knowledge-pack/keys/dev-2026-11.pub"),
)];

/// The pack shipped with this build.
pub const BUNDLED_PACK: &str = include_str!("../../../knowledge-pack/pack.json");
/// Its signature.
pub const BUNDLED_SIG: &str = include_str!("../../../knowledge-pack/pack.json.sig");

/// An official source a rule was taken from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    /// Date the rule was checked against the source (YYYY-MM-DD).
    pub checked: String,
}

/// "Gradle `min_gradle` is the first version that can run on Java `java`."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleJava {
    pub java: u32,
    pub min_gradle: String,
    pub source: String,
}

/// Requirements of one Android Gradle Plugin minor line (e.g. `8.13`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agp {
    pub agp: String,
    pub min_gradle: String,
    pub min_jdk: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_build_tools: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_ndk: Option<String>,
    pub source: String,
}

/// Minimum AGP needed to compile against an API level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiMinAgp {
    pub api: String,
    pub min_agp: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpoSdk {
    pub sdk: u32,
    pub react_native: String,
    pub min_node: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactNative {
    pub react_native: String,
    pub min_node: String,
    pub recommended_jdk: u32,
    pub source: String,
}

/// An installable component: approximate download size and licence, for the install plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub title: String,
    /// Approximate download size in bytes (exact sizes come from the download metadata at install time).
    pub approx_download_bytes: u64,
    /// Approximate size on disk after installation.
    pub approx_disk_bytes: u64,
    /// Licence id; `None` = no acceptance step needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Licence {
    pub title: String,
    pub url: String,
    /// Must the user explicitly accept it before installation?
    pub requires_acceptance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub jdk_major: u32,
    pub system_image_tag: String,
    pub system_image_abi: String,
    pub node_major: u32,
    /// SDK package names that do not follow `platforms/android-<api>` (observed in practice).
    pub platform_package_overrides: BTreeMap<String, String>,
    /// `CMake` version AGP installs when a project uses `CMake` without naming a version.
    pub default_cmake: String,
    pub source: String,
}

/// The whole pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema: u32,
    pub pack_version: String,
    pub sources: BTreeMap<String, Source>,
    pub gradle_java: Vec<GradleJava>,
    pub agp: Vec<Agp>,
    pub api_min_agp: Vec<ApiMinAgp>,
    pub expo_sdk: Vec<ExpoSdk>,
    pub react_native: Vec<ReactNative>,
    pub components: BTreeMap<String, Component>,
    pub licences: BTreeMap<String, Licence>,
    pub defaults: Defaults,
}

/// Why a pack was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    Parse(String),
    BadSignature(String),
    Invalid(Vec<String>),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "knowledge pack is not valid JSON: {e}"),
            Self::BadSignature(e) => write!(f, "knowledge pack signature check failed: {e}"),
            Self::Invalid(errs) => write!(f, "knowledge pack is invalid: {}", errs.join("; ")),
        }
    }
}

impl std::error::Error for PackError {}

impl Pack {
    /// Verifies the signature over the exact bytes, then parses and validates.
    pub fn load_verified(json: &str, signature_b64: &str) -> Result<Self, PackError> {
        verify_signature(json.as_bytes(), signature_b64)?;
        let pack: Pack = serde_json::from_str(json).map_err(|e| PackError::Parse(e.to_string()))?;
        let errors = pack.validate();
        if errors.is_empty() {
            Ok(pack)
        } else {
            Err(PackError::Invalid(errors))
        }
    }

    /// The pack built into this binary.
    pub fn bundled() -> Result<Self, PackError> {
        Self::load_verified(BUNDLED_PACK, BUNDLED_SIG)
    }

    /// Semantic checks; returns all problems found.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        if self.schema != SCHEMA {
            e.push(format!(
                "schema {} not supported (expected {SCHEMA})",
                self.schema
            ));
        }
        for (id, s) in &self.sources {
            if !s.url.starts_with("https://") {
                e.push(format!("source {id}: url must be https"));
            }
            if s.checked.len() != 10 {
                e.push(format!("source {id}: checked must be YYYY-MM-DD"));
            }
        }
        let sources = &self.sources;
        let src = |e: &mut Vec<String>, rule: &str, id: &str| {
            if !sources.contains_key(id) {
                e.push(format!("{rule}: unknown source '{id}'"));
            }
        };
        let mut ver = Vec::new();
        for r in &self.gradle_java {
            src(&mut e, &format!("gradle_java {}", r.java), &r.source);
            ver.push((format!("gradle_java {}", r.java), r.min_gradle.clone()));
        }
        for r in &self.agp {
            src(&mut e, &format!("agp {}", r.agp), &r.source);
            ver.push((format!("agp {}", r.agp), r.agp.clone()));
            ver.push((format!("agp {} min_gradle", r.agp), r.min_gradle.clone()));
        }
        for r in &self.api_min_agp {
            src(&mut e, &format!("api_min_agp {}", r.api), &r.source);
            ver.push((format!("api_min_agp {}", r.api), r.min_agp.clone()));
        }
        for r in &self.expo_sdk {
            src(&mut e, &format!("expo_sdk {}", r.sdk), &r.source);
            ver.push((format!("expo_sdk {}", r.sdk), r.min_node.clone()));
        }
        for r in &self.react_native {
            src(
                &mut e,
                &format!("react_native {}", r.react_native),
                &r.source,
            );
            ver.push((
                format!("react_native {}", r.react_native),
                r.min_node.clone(),
            ));
        }
        for (id, c) in &self.components {
            src(&mut e, &format!("component {id}"), &c.source);
            if let Some(l) = &c.licence {
                if !self.licences.contains_key(l) {
                    e.push(format!("component {id}: unknown licence '{l}'"));
                }
            }
        }
        src(&mut e, "defaults", &self.defaults.source);
        for (what, v) in ver {
            if Version::parse(&v).is_none() {
                e.push(format!("{what}: '{v}' is not a version"));
            }
        }
        let dup = |keys: Vec<String>, what: &str, e: &mut Vec<String>| {
            let mut seen = HashSet::new();
            for k in keys {
                if !seen.insert(k.clone()) {
                    e.push(format!("duplicate {what} '{k}'"));
                }
            }
        };
        dup(
            self.gradle_java
                .iter()
                .map(|r| r.java.to_string())
                .collect(),
            "gradle_java",
            &mut e,
        );
        dup(
            self.agp.iter().map(|r| r.agp.clone()).collect(),
            "agp",
            &mut e,
        );
        dup(
            self.api_min_agp.iter().map(|r| r.api.clone()).collect(),
            "api_min_agp",
            &mut e,
        );
        dup(
            self.expo_sdk.iter().map(|r| r.sdk.to_string()).collect(),
            "expo_sdk",
            &mut e,
        );
        e
    }
}

/// Checks a base64 Ed25519 signature against the trusted keys.
pub fn verify_signature(data: &[u8], signature_b64: &str) -> Result<(), PackError> {
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64.trim())
        .map_err(|e| PackError::BadSignature(format!("signature is not base64: {e}")))?;
    let sig =
        Signature::from_slice(&sig_bytes).map_err(|e| PackError::BadSignature(e.to_string()))?;
    for (_, key_b64) in TRUSTED_KEYS {
        if let Some(key) = decode_public_key(key_b64) {
            if key.verify(data, &sig).is_ok() {
                return Ok(());
            }
        }
    }
    Err(PackError::BadSignature("no trusted key matches".into()))
}

fn decode_public_key(b64: &str) -> Option<VerifyingKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()
}

/// Creates a new signing key; returns (secret seed b64, public key b64).
pub fn generate_keypair() -> std::io::Result<(String, String)> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| std::io::Error::other(e.to_string()))?;
    let key = SigningKey::from_bytes(&seed);
    let b64 = &base64::engine::general_purpose::STANDARD;
    Ok((b64.encode(seed), b64.encode(key.verifying_key().to_bytes())))
}

/// Signs data with a base64 secret seed; returns the base64 signature.
pub fn sign(data: &[u8], secret_b64: &str) -> Result<String, String> {
    let seed = base64::engine::general_purpose::STANDARD
        .decode(secret_b64.trim())
        .map_err(|e| e.to_string())?;
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| "secret key must be 32 bytes".to_string())?;
    let sig = SigningKey::from_bytes(&seed).sign(data);
    Ok(base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pack_verifies_and_validates() {
        let pack = Pack::bundled().expect("bundled pack must verify and validate");
        assert_eq!(pack.schema, SCHEMA);
        assert!(!pack.agp.is_empty() && !pack.gradle_java.is_empty());
    }

    #[test]
    fn tampered_pack_is_rejected() {
        let tampered = BUNDLED_PACK.replacen("\"min_jdk\": 17", "\"min_jdk\": 11", 1);
        assert_ne!(tampered, BUNDLED_PACK);
        assert!(matches!(
            Pack::load_verified(&tampered, BUNDLED_SIG),
            Err(PackError::BadSignature(_))
        ));
    }

    #[test]
    fn garbage_signature_is_rejected() {
        assert!(matches!(
            Pack::load_verified(BUNDLED_PACK, "not base64!"),
            Err(PackError::BadSignature(_))
        ));
        assert!(matches!(
            Pack::load_verified(BUNDLED_PACK, "AAAA"),
            Err(PackError::BadSignature(_))
        ));
    }

    #[test]
    fn sign_and_verify_round_trip_with_fresh_key() {
        let (secret, public) = generate_keypair().unwrap();
        let sig = sign(b"hello", &secret).unwrap();
        let key = decode_public_key(&public).unwrap();
        let s = Signature::from_slice(
            &base64::engine::general_purpose::STANDARD
                .decode(sig)
                .unwrap(),
        )
        .unwrap();
        assert!(key.verify(b"hello", &s).is_ok());
        assert!(key.verify(b"hellO", &s).is_err());
        // a fresh key is not trusted
        assert!(verify_signature(b"hello", &sign(b"hello", &secret).unwrap()).is_err());
    }

    #[test]
    fn validation_finds_problems() {
        let mut pack: Pack = serde_json::from_str(BUNDLED_PACK).unwrap();
        pack.agp[0].source = "nope".into();
        pack.agp[0].min_gradle = "latest".into();
        pack.gradle_java.push(pack.gradle_java[0].clone());
        let errs = pack.validate();
        assert!(
            errs.iter().any(|e| e.contains("unknown source 'nope'")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("'latest' is not a version")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("duplicate gradle_java")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let bad = BUNDLED_PACK.replacen("\"schema\": 1", "\"schema\": 1, \"surprise\": true", 1);
        assert!(serde_json::from_str::<Pack>(&bad).is_err());
    }
}
