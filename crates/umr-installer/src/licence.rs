//! INS-004: licences that must be accepted before installation.
//!
//! Android CLI accepts the Android SDK licences silently and writes `licenses/` itself
//! (POC-001 finding 2). The product therefore shows the full licence text first and records
//! the user's acceptance bound to the SHA-256 of that exact text. If Google changes the text,
//! the hash changes and the user is asked again. No install step runs without it
//! ([`require_accepted`] is checked by the engine, not only by the UI).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use umr_storage::Db;

use crate::catalog::Repository;
use crate::verify::sha256_hex;
use crate::InstallError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenceText {
    /// Google licence id, e.g. `android-sdk-license`.
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Full text exactly as published.
    pub text: String,
    pub sha256: String,
    /// Already accepted by the user (this exact text).
    pub accepted: bool,
}

/// Human title of a Google licence id.
#[must_use]
pub fn title(id: &str) -> String {
    match id {
        "android-sdk-license" => "Android Software Development Kit License Agreement".into(),
        "android-sdk-preview-license" => "Android SDK Preview License Agreement".into(),
        "android-sdk-arm-dbt-license" => "Android SDK ARM DBT License Agreement".into(),
        "intel-android-sysimage-license" => "Intel Android System Image License Agreement".into(),
        other => other.into(),
    }
}

/// Licence text from a repository manifest.
#[must_use]
pub fn from_repository(repo: &Repository, id: &str) -> Option<LicenceText> {
    let text = repo.licences.get(id)?;
    Some(LicenceText {
        id: id.into(),
        title: title(id),
        url: (id == "android-sdk-license")
            .then(|| "https://developer.android.com/studio/terms".to_string()),
        sha256: sha256_hex(text.as_bytes()),
        text: text.clone(),
        accepted: false,
    })
}

fn lock(db: &Mutex<Db>) -> std::sync::MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Fills `accepted` from the database.
pub fn mark_accepted(db: &Mutex<Db>, list: &mut [LicenceText]) -> Result<(), InstallError> {
    let db = lock(db);
    for l in list {
        l.accepted = db.licence_accepted(&l.id, &l.sha256)?;
    }
    Ok(())
}

/// Fails with the ids of licences whose current text is not accepted.
pub fn require_accepted(db: &Mutex<Db>, list: &[LicenceText]) -> Result<(), InstallError> {
    let db = lock(db);
    let mut missing = Vec::new();
    for l in list {
        if !db.licence_accepted(&l.id, &l.sha256)? {
            missing.push(l.id.clone());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(InstallError::LicenceNotAccepted(missing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{parse_repository, tests::REPO};
    use crate::testkit::tmp_dir;

    #[test]
    fn acceptance_is_bound_to_the_exact_text() {
        let d = tmp_dir("licence");
        let db = Mutex::new(Db::open_in(&d).unwrap());
        let repo = parse_repository(REPO).unwrap();
        let mut l = vec![from_repository(&repo, "android-sdk-license").unwrap()];
        assert_eq!(
            l[0].title,
            "Android Software Development Kit License Agreement"
        );
        assert!(l[0].text.starts_with("Terms and Conditions"));
        assert_eq!(
            require_accepted(&db, &l),
            Err(InstallError::LicenceNotAccepted(vec![
                "android-sdk-license".into()
            ]))
        );
        lock(&db)
            .accept_licence(&l[0].id, &l[0].sha256, Some("admin"), 1)
            .unwrap();
        require_accepted(&db, &l).unwrap();
        mark_accepted(&db, &mut l).unwrap();
        assert!(l[0].accepted);
        // Google changes the text: asked again
        let mut changed = l[0].clone();
        changed.text.push_str(" (updated)");
        changed.sha256 = sha256_hex(changed.text.as_bytes());
        assert!(require_accepted(&db, &[changed]).is_err());
        assert!(from_repository(&repo, "nope").is_none());
    }
}
