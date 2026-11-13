//! REL-002: updates of the product itself.
//!
//! The release pipeline (`scripts/release.ps1`) publishes `releases.json` next to the files; the
//! website's download page reads the same manifest (Phase 9, DIST-002). `umr update` fetches it
//! (HTTPS; the manifest's host and GitHub's release CDN only), picks the newest release on the
//! configured channel that is newer than the running version, downloads its installer with the
//! resumable verified downloader (size + SHA-256 from the manifest must match before anything
//! runs) and leaves running it to the caller.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::http::{Http, Policy};
use crate::verify::Checksum;
use crate::InstallError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub product: String,
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub min_windows_build: u32,
    #[serde(default)]
    pub signed: bool,
    #[serde(default)]
    pub notes_url: String,
    pub files: Vec<ReleaseFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFile {
    /// `installer`, `portable` or `cli`.
    pub kind: String,
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

impl Release {
    #[must_use]
    pub fn file(&self, kind: &str) -> Option<&ReleaseFile> {
        self.files.iter().find(|f| f.kind == kind)
    }
}

/// `major.minor.patch[-pre]` as comparable parts; a pre-release sorts before its release.
#[must_use]
pub fn version_key(v: &str) -> Option<(u64, u64, u64, bool, String)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = v.split_once('-').map_or((v, ""), |(c, p)| (c, p));
    let mut it = core.split('.');
    let n = |s: Option<&str>| s.and_then(|x| x.parse::<u64>().ok());
    let key = (
        n(it.next())?,
        n(it.next())?,
        n(it.next())?,
        pre.is_empty(),
        pre.to_string(),
    );
    it.next().is_none().then_some(key)
}

/// The newest release newer than `current` on `channel` (`beta` also takes stable releases),
/// that this Windows build can run.
#[must_use]
pub fn pick<'a>(
    m: &'a Manifest,
    current: &str,
    channel: &str,
    windows_build: u32,
) -> Option<&'a Release> {
    let cur = version_key(current)?;
    m.releases
        .iter()
        .filter(|r| r.channel == "stable" || (channel == "beta" && r.channel == "beta"))
        .filter(|r| r.min_windows_build <= windows_build)
        .filter_map(|r| version_key(&r.version).map(|k| (k, r)))
        .filter(|(k, _)| *k > cur)
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| r)
}

/// Hosts an update may come from: the manifest's own host and GitHub's release CDN.
pub fn policy_for(manifest_url: &str) -> Result<Policy, InstallError> {
    let u = Url::parse(manifest_url)
        .map_err(|e| InstallError::HostNotAllowed(format!("manifest URL: {e}")))?;
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let loopback = u.scheme() == "http" && (host == "127.0.0.1" || host == "localhost");
    if u.scheme() != "https" && !loopback {
        return Err(InstallError::Metadata(format!(
            "the update manifest must be HTTPS ({manifest_url})"
        )));
    }
    let mut p = Policy::official();
    p.hosts = vec![host, "github.com".into(), ".githubusercontent.com".into()];
    p.allow_loopback_http = loopback;
    Ok(p)
}

pub fn fetch_manifest(manifest_url: &str) -> Result<Manifest, InstallError> {
    let http = Http::new(policy_for(manifest_url)?)?;
    let text = http.get_text(manifest_url, 1 << 20)?;
    let m: Manifest = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| InstallError::Metadata(format!("the release manifest is not valid: {e}")))?;
    if m.schema != 1 {
        return Err(InstallError::Metadata(format!(
            "release manifest schema {} is newer than this version understands; download the update from the website",
            m.schema
        )));
    }
    Ok(m)
}

/// Downloads `f` into `dir` and verifies it (size and SHA-256). Returns the file.
pub fn download(manifest_url: &str, f: &ReleaseFile, dir: &Path) -> Result<PathBuf, InstallError> {
    if f.name.contains(['/', '\\', ':']) || f.name.starts_with('.') {
        return Err(InstallError::Metadata(format!(
            "bad file name in the manifest: {}",
            f.name
        )));
    }
    let http = Http::new(policy_for(manifest_url)?)?;
    let dest = dir.join(&f.name);
    let cancel = AtomicBool::new(false);
    http.download(
        &f.url,
        &dest,
        &Checksum::Sha256(f.sha256.to_ascii_lowercase()),
        Some(f.size),
        &mut |_, _| {},
        &cancel,
    )?;
    if dest.metadata()?.len() != f.size {
        let _ = std::fs::remove_file(&dest);
        return Err(InstallError::Metadata(format!(
            "{}: size does not match the manifest",
            f.name
        )));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(v: &str, ch: &str, min: u32) -> Release {
        Release {
            version: v.into(),
            channel: ch.into(),
            date: String::new(),
            min_windows_build: min,
            signed: false,
            notes_url: String::new(),
            files: vec![],
        }
    }

    #[test]
    fn versions_compare_like_semver() {
        assert!(version_key("1.10.0") > version_key("1.9.9"));
        assert!(version_key("1.0.0") > version_key("1.0.0-rc.1"));
        assert!(version_key("v0.2.0") > version_key("0.1.9"));
        assert_eq!(version_key("1.0"), None);
        assert_eq!(version_key("1.0.0.0"), None);
    }

    #[test]
    fn pick_respects_channel_version_and_windows() {
        let m = Manifest {
            schema: 1,
            product: String::new(),
            releases: vec![
                rel("0.2.0", "stable", 22000),
                rel("0.3.0-beta.1", "beta", 22000),
                rel("0.4.0", "stable", 99999),
                rel("0.1.0", "stable", 0),
            ],
        };
        assert_eq!(
            pick(&m, "0.1.0", "stable", 26100).map(|r| r.version.as_str()),
            Some("0.2.0")
        );
        assert_eq!(
            pick(&m, "0.1.0", "beta", 26100).map(|r| r.version.as_str()),
            Some("0.3.0-beta.1")
        );
        assert_eq!(pick(&m, "0.2.0", "stable", 26100), None, "nothing newer");
        assert_eq!(pick(&m, "0.1.0", "stable", 100), None, "too old a Windows");
    }

    #[test]
    fn only_https_or_loopback_manifests() {
        assert!(policy_for("https://example.org/releases.json").is_ok());
        assert!(
            policy_for("http://127.0.0.1:8000/releases.json")
                .unwrap()
                .allow_loopback_http
        );
        assert!(policy_for("http://example.org/releases.json").is_err());
        assert!(policy_for("file:///C:/x.json").is_err());
        let p = policy_for("https://get.example.org/r.json").unwrap();
        assert!(p.allows(&Url::parse("https://get.example.org/v1/setup.exe").unwrap()));
        assert!(p.allows(&Url::parse("https://objects.githubusercontent.com/x").unwrap()));
        assert!(!p.allows(&Url::parse("https://evil.example.com/setup.exe").unwrap()));
    }

    #[test]
    fn the_release_script_manifest_parses() {
        let m: Manifest = serde_json::from_str(r#"{"schema":1,"product":"AI Mobile Runtime","releases":[{"version":"0.1.0","channel":"stable","date":"2026-11-13","min_windows_build":22000,"signed":false,"notes_url":"u","files":[{"kind":"installer","name":"a.exe","url":"https://x/a.exe","size":1,"sha256":"ab"}]}]}"#).unwrap();
        assert_eq!(m.releases[0].file("installer").unwrap().name, "a.exe");
    }
}
