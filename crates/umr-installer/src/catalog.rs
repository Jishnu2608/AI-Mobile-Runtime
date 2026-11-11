//! Official release metadata → concrete download (URL, checksum, size, version).
//!
//! | Component | Metadata | Checksum |
//! |---|---|---|
//! | JDK | Adoptium API `v3/assets/latest/<major>/hotspot` | SHA-256 |
//! | Node.js | `nodejs.org/dist/index.json` + `SHASUMS256.txt` | SHA-256 |
//! | Flutter | `releases_windows.json` | SHA-256 |
//! | Android cmdline-tools, licences | Google `repository2-3.xml` (`sys-img2-3.xml` for images) | SHA-1 |
//!
//! Every URL is fetched through [`crate::http::Http`], i.e. HTTPS on official hosts only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::verify::Checksum;
use crate::InstallError;

pub const GOOGLE_REPOSITORY: &str = "https://dl.google.com/android/repository/";
pub const GOOGLE_REPOSITORY_XML: &str =
    "https://dl.google.com/android/repository/repository2-3.xml";
pub const NODE_DIST: &str = "https://nodejs.org/dist";
pub const FLUTTER_RELEASES: &str =
    "https://storage.googleapis.com/flutter_infra_release/releases/releases_windows.json";

/// Metadata documents are small; refuse anything bigger.
pub const MAX_METADATA_BYTES: u64 = 32 * 1024 * 1024;

/// One file to download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub file_name: String,
    pub checksum: Checksum,
    pub size: Option<u64>,
    pub version: String,
}

fn meta(e: impl std::fmt::Display) -> InstallError {
    InstallError::Metadata(e.to_string())
}

// ---- Adoptium (Temurin JDK) ---------------------------------------------------------------

pub const ADOPTIUM_API: &str = "https://api.adoptium.net";

#[must_use]
pub fn adoptium_url(api: &str, major: u32) -> String {
    format!("{}/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jdk&os=windows&vendor=eclipse", api.trim_end_matches('/'))
}

/// Picks the Windows x64 JDK zip from an Adoptium `assets/latest` answer.
pub fn parse_adoptium(json: &str) -> Result<Artifact, InstallError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(meta)?;
    let list = v
        .as_array()
        .ok_or_else(|| meta("Adoptium answer is not a list"))?;
    for a in list {
        let b = &a["binary"];
        let p = &b["package"];
        let name = p["name"].as_str().unwrap_or_default();
        if b["os"] != "windows"
            || b["architecture"] != "x64"
            || b["image_type"] != "jdk"
            || !std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            continue;
        }
        let (Some(link), Some(sum)) = (p["link"].as_str(), p["checksum"].as_str()) else {
            continue;
        };
        let version = a["version"]["openjdk_version"]
            .as_str()
            .or_else(|| a["version"]["semver"].as_str())
            .unwrap_or(name);
        return Ok(Artifact {
            url: link.into(),
            file_name: name.into(),
            checksum: Checksum::Sha256(sum.to_lowercase()),
            size: p["size"].as_u64(),
            version: version.into(),
        });
    }
    Err(meta("no Windows x64 JDK zip in the Adoptium answer"))
}

// ---- Node.js ------------------------------------------------------------------------------

/// Newest release of a major line with a Windows x64 zip (index.json is newest first).
/// Pre-releases (`-alpha`, `-rc`) are skipped.
pub fn parse_node_index(json: &str, major: u64) -> Result<String, InstallError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(meta)?;
    v.as_array()
        .into_iter()
        .flatten()
        .find_map(|r| {
            let ver = r["version"].as_str()?.trim_start_matches('v');
            let has_zip = r["files"].as_array()?.iter().any(|f| f == "win-x64-zip");
            let maj: u64 = ver.split('.').next()?.parse().ok()?;
            (has_zip && maj == major && !ver.contains('-')).then(|| ver.to_string())
        })
        .ok_or_else(|| {
            meta(format!(
                "no Node.js {major}.x release with a Windows x64 zip"
            ))
        })
}

#[must_use]
pub fn node_file(version: &str) -> String {
    format!("node-v{version}-win-x64.zip")
}

#[must_use]
pub fn node_shasums_url(dist: &str, version: &str) -> String {
    format!("{}/v{version}/SHASUMS256.txt", dist.trim_end_matches('/'))
}

/// The Node zip artifact from `SHASUMS256.txt`.
pub fn node_artifact(dist: &str, version: &str, shasums: &str) -> Result<Artifact, InstallError> {
    let file = node_file(version);
    let sum = shasums
        .lines()
        .find_map(|l| {
            let (h, f) = l.split_once(char::is_whitespace)?;
            (f.trim() == file).then(|| h.trim().to_lowercase())
        })
        .ok_or_else(|| meta(format!("{file} not listed in SHASUMS256.txt")))?;
    Ok(Artifact {
        url: format!("{}/v{version}/{file}", dist.trim_end_matches('/')),
        file_name: file,
        checksum: Checksum::Sha256(sum),
        size: None,
        version: version.into(),
    })
}

// ---- Flutter ------------------------------------------------------------------------------

/// Stable Flutter release: the exact `pinned` version, else the current stable if it is at
/// least `min`, else the newest stable at least `min`.
pub fn parse_flutter(
    json: &str,
    pinned: Option<&str>,
    min: Option<&str>,
) -> Result<Artifact, InstallError> {
    use umr_resolver::version::Version;
    let v: serde_json::Value = serde_json::from_str(json).map_err(meta)?;
    let base = v["base_url"].as_str().ok_or_else(|| meta("no base_url"))?;
    let current = v["current_release"]["stable"].as_str();
    let releases: Vec<&serde_json::Value> = v["releases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["channel"] == "stable")
        .collect();
    let min_v = min.and_then(Version::parse);
    let ok_min =
        |r: &serde_json::Value| match (&min_v, r["version"].as_str().and_then(Version::parse)) {
            (Some(m), Some(rv)) => rv >= *m,
            (None, _) => true,
            (Some(_), None) => false,
        };
    let pick = if let Some(p) = pinned {
        releases.iter().find(|r| r["version"] == p).copied()
    } else {
        releases
            .iter()
            .find(|r| current.is_some_and(|c| r["hash"] == c) && ok_min(r))
            .or_else(|| {
                releases.iter().filter(|r| ok_min(r)).max_by(|a, b| {
                    let va = a["version"].as_str().and_then(Version::parse);
                    let vb = b["version"].as_str().and_then(Version::parse);
                    va.cmp(&vb)
                })
            })
            .copied()
    };
    let r = pick.ok_or_else(|| {
        meta(format!(
            "no stable Flutter release matches {}",
            pinned.or(min).unwrap_or("stable")
        ))
    })?;
    let archive = r["archive"]
        .as_str()
        .ok_or_else(|| meta("release without archive"))?;
    let sum = r["sha256"]
        .as_str()
        .ok_or_else(|| meta("release without sha256"))?;
    Ok(Artifact {
        url: format!("{}/{archive}", base.trim_end_matches('/')),
        file_name: archive.rsplit('/').next().unwrap_or(archive).into(),
        checksum: Checksum::Sha256(sum.to_lowercase()),
        size: None,
        version: r["version"].as_str().unwrap_or_default().into(),
    })
}

// ---- Google SDK repository ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoArchive {
    pub url: String,
    pub size: Option<u64>,
    pub sha1: String,
    pub host_os: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPackage {
    /// `;`-separated, e.g. `platforms;android-36`.
    pub path: String,
    pub revision: String,
    /// `stable`, `beta`, `dev`, `canary`.
    pub channel: String,
    pub licence: Option<String>,
    pub archives: Vec<RepoArchive>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Repository {
    /// Licence id → full text, exactly as published.
    pub licences: BTreeMap<String, String>,
    pub packages: Vec<RepoPackage>,
}

/// URL of the system-image manifest for a tag (`google_apis`).
#[must_use]
pub fn sysimg_url(base: &str, tag: &str) -> String {
    format!(
        "{}/sys-img/{tag}/sys-img2-3.xml",
        base.trim_end_matches('/')
    )
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

fn text(n: Option<roxmltree::Node<'_, '_>>) -> Option<String> {
    n.and_then(|c| c.text()).map(|t| t.trim().to_string())
}

/// Parses a Google SDK repository manifest (`repository2-3.xml`, `sys-img2-3.xml`).
pub fn parse_repository(xml: &str) -> Result<Repository, InstallError> {
    let doc = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 20_000_000,
        },
    )
    .map_err(meta)?;
    let root = doc.root_element();
    let mut repo = Repository::default();
    let mut channels: BTreeMap<String, String> = BTreeMap::new();
    for n in root.children().filter(roxmltree::Node::is_element) {
        match n.tag_name().name() {
            "license" => {
                if let Some(id) = n.attribute("id") {
                    repo.licences
                        .insert(id.into(), n.text().unwrap_or_default().to_string());
                }
            }
            "channel" => {
                if let Some(id) = n.attribute("id") {
                    channels.insert(id.into(), n.text().unwrap_or_default().trim().to_string());
                }
            }
            _ => {}
        }
    }
    for p in root
        .children()
        .filter(|n| n.tag_name().name() == "remotePackage")
    {
        let Some(path) = p.attribute("path") else {
            continue;
        };
        let rev = child(p, "revision").map_or_else(String::new, |r| {
            ["major", "minor", "micro"]
                .iter()
                .filter_map(|k| text(child(r, k)))
                .collect::<Vec<_>>()
                .join(".")
        });
        let channel = child(p, "channelRef")
            .and_then(|c| c.attribute("ref"))
            .and_then(|r| channels.get(r).cloned())
            .unwrap_or_else(|| "stable".into());
        let licence = child(p, "uses-license")
            .and_then(|l| l.attribute("ref"))
            .map(String::from);
        let mut archives = Vec::new();
        if let Some(list) = child(p, "archives") {
            for a in list.children().filter(|c| c.tag_name().name() == "archive") {
                let Some(c) = child(a, "complete") else {
                    continue;
                };
                let (Some(url), Some(sha1)) = (text(child(c, "url")), text(child(c, "checksum")))
                else {
                    continue;
                };
                archives.push(RepoArchive {
                    url,
                    size: text(child(c, "size")).and_then(|s| s.parse().ok()),
                    sha1: sha1.to_lowercase(),
                    host_os: text(child(a, "host-os")),
                });
            }
        }
        repo.packages.push(RepoPackage {
            path: path.into(),
            revision: rev,
            channel,
            licence,
            archives,
        });
    }
    if repo.packages.is_empty() {
        return Err(meta("SDK repository manifest lists no packages"));
    }
    Ok(repo)
}

impl Repository {
    /// Stable package by path; accepts `/` or `;` separators.
    #[must_use]
    pub fn package(&self, path: &str) -> Option<&RepoPackage> {
        let want = path.replace('/', ";");
        self.packages
            .iter()
            .find(|p| p.path == want && p.channel == "stable")
            .or_else(|| self.packages.iter().find(|p| p.path == want))
    }

    /// Windows download of a package (archives without host-os are for all hosts).
    /// `base` is the folder relative archive URLs are resolved against
    /// ([`GOOGLE_REPOSITORY`] for the official repository).
    pub fn windows_artifact(&self, base: &str, path: &str) -> Result<Artifact, InstallError> {
        let p = self
            .package(path)
            .ok_or_else(|| meta(format!("{path} is not in the SDK repository")))?;
        let a = p
            .archives
            .iter()
            .find(|a| a.host_os.as_deref() == Some("windows"))
            .or_else(|| p.archives.iter().find(|a| a.host_os.is_none()))
            .ok_or_else(|| meta(format!("{path} has no Windows download")))?;
        let url = if a.url.starts_with("https://") {
            a.url.clone()
        } else {
            format!("{}/{}", base.trim_end_matches('/'), a.url)
        };
        Ok(Artifact {
            file_name: url.rsplit('/').next().unwrap_or_default().into(),
            url,
            checksum: Checksum::Sha1(a.sha1.clone()),
            size: a.size,
            version: p.revision.clone(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const ADOPTIUM: &str = r#"[{"binary":{"architecture":"x64","image_type":"jdk","os":"windows",
        "installer":{"checksum":"9f","link":"https://github.com/adoptium/x.msi","name":"OpenJDK17U-jdk_x64_windows_hotspot_17.0.21_9.msi","size":1},
        "package":{"checksum":"98518ECF554465057ee5509462631deeee374bc368c12a411d225cd29381f84b","link":"https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.21%2B9/OpenJDK17U-jdk_x64_windows_hotspot_17.0.21_9.zip","name":"OpenJDK17U-jdk_x64_windows_hotspot_17.0.21_9.zip","size":191117380}},
        "release_name":"jdk-17.0.21+9","version":{"major":17,"openjdk_version":"17.0.21+9","semver":"17.0.21+9"}}]"#;

    pub const REPO: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<sdk:sdk-repository xmlns:sdk="http://schemas.android.com/sdk/android/repo/repository2/03" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <license id="android-sdk-license" type="text">Terms and Conditions

This is the Android Software Development Kit License Agreement &amp; more.</license>
  <license id="android-sdk-preview-license" type="text">Preview terms</license>
  <channel id="channel-0">stable</channel>
  <channel id="channel-3">canary</channel>
  <remotePackage path="cmdline-tools;latest">
    <revision><major>24</major><minor>0</minor></revision>
    <uses-license ref="android-sdk-preview-license"/>
    <channelRef ref="channel-3"/>
    <archives><archive><complete><size>1</size><checksum type="sha1">ff</checksum><url>commandlinetools-win-99_latest.zip</url></complete><host-os>windows</host-os></archive></archives>
  </remotePackage>
  <remotePackage path="cmdline-tools;latest">
    <revision><major>23</major><minor>0</minor></revision>
    <uses-license ref="android-sdk-license"/>
    <channelRef ref="channel-0"/>
    <archives>
      <archive><complete><size>181052239</size><checksum type="sha1">e0</checksum><url>commandlinetools-linux-16111833_latest.zip</url></complete><host-os>linux</host-os></archive>
      <archive><complete><size>154957218</size><checksum type="sha1">57D04F2D75EB8E8FFFC5000A987E5DE4B5A63E9D</checksum><url>commandlinetools-win-16111833_latest.zip</url></complete><host-os>windows</host-os></archive>
    </archives>
  </remotePackage>
  <remotePackage path="platforms;android-36">
    <revision><major>2</major></revision>
    <uses-license ref="android-sdk-license"/>
    <channelRef ref="channel-0"/>
    <archives><archive><complete><size>65</size><checksum type="sha1">aa</checksum><url>platform-36_r02.zip</url></complete></archive></archives>
  </remotePackage>
</sdk:sdk-repository>"#;

    #[test]
    fn adoptium_zip_is_picked() {
        let a = parse_adoptium(ADOPTIUM).unwrap();
        assert!(
            a.url.to_ascii_lowercase().ends_with(".zip")
                && a.url.starts_with("https://github.com/adoptium/")
        );
        assert_eq!(a.version, "17.0.21+9");
        assert_eq!(a.size, Some(191_117_380));
        assert_eq!(
            a.checksum,
            Checksum::Sha256(
                "98518ecf554465057ee5509462631deeee374bc368c12a411d225cd29381f84b".into()
            )
        );
        assert!(parse_adoptium("[]").is_err());
        assert!(parse_adoptium("{").is_err());
    }

    #[test]
    fn node_release_and_checksum() {
        let idx = r#"[{"version":"v27.0.0-alpha.0","files":["win-x64-zip"]},{"version":"v26.13.1","files":["win-x64-zip"]},
            {"version":"v24.15.1","files":["linux-x64"]},{"version":"v24.15.0","files":["win-x64-zip","win-x64-msi"]},{"version":"v24.14.0","files":["win-x64-zip"]}]"#;
        assert_eq!(parse_node_index(idx, 24).unwrap(), "24.15.0");
        assert!(parse_node_index(idx, 27).is_err(), "pre-release skipped");
        let sums = "aa  node-v24.15.0-win-arm64.zip\ncc5149eabd53779ce1e7bdc5401643622d0c7e6800ade18928a767e940bb0e62  node-v24.15.0-win-x64.zip\n";
        let a = node_artifact(NODE_DIST, "24.15.0", sums).unwrap();
        assert_eq!(
            a.url,
            "https://nodejs.org/dist/v24.15.0/node-v24.15.0-win-x64.zip"
        );
        assert_eq!(
            a.checksum.hex(),
            "cc5149eabd53779ce1e7bdc5401643622d0c7e6800ade18928a767e940bb0e62"
        );
        assert!(node_artifact(NODE_DIST, "24.1.0", sums).is_err());
    }

    #[test]
    fn flutter_release_choice() {
        let j = r#"{"base_url":"https://storage.googleapis.com/flutter_infra_release/releases","current_release":{"stable":"h3"},
          "releases":[{"hash":"h3","channel":"stable","version":"3.50.0","archive":"stable/windows/flutter_windows_3.50.0-stable.zip","sha256":"A1"},
                      {"hash":"hb","channel":"beta","version":"3.51.0-0.1.pre","archive":"beta/windows/b.zip","sha256":"b"},
                      {"hash":"h2","channel":"stable","version":"3.38.1","archive":"stable/windows/flutter_windows_3.38.1-stable.zip","sha256":"a2"}]}"#;
        let cur = parse_flutter(j, None, None).unwrap();
        assert_eq!((cur.version.as_str(), cur.checksum.hex()), ("3.50.0", "a1"));
        assert_eq!(cur.url, "https://storage.googleapis.com/flutter_infra_release/releases/stable/windows/flutter_windows_3.50.0-stable.zip");
        assert_eq!(
            parse_flutter(j, Some("3.38.1"), None).unwrap().version,
            "3.38.1"
        );
        assert_eq!(
            parse_flutter(j, None, Some("3.35.0")).unwrap().version,
            "3.50.0"
        );
        assert!(parse_flutter(j, None, Some("4.0.0")).is_err());
        assert!(
            parse_flutter(j, Some("3.51.0-0.1.pre"), None).is_err(),
            "beta never chosen"
        );
    }

    #[test]
    fn google_repository() {
        let r = parse_repository(REPO).unwrap();
        assert!(r.licences["android-sdk-license"].contains("License Agreement & more"));
        let p = r.package("cmdline-tools/latest").unwrap();
        assert_eq!(
            (
                p.channel.as_str(),
                p.revision.as_str(),
                p.licence.as_deref()
            ),
            ("stable", "23.0", Some("android-sdk-license"))
        );
        let a = r
            .windows_artifact(GOOGLE_REPOSITORY, "cmdline-tools;latest")
            .unwrap();
        assert_eq!(
            a.url,
            "https://dl.google.com/android/repository/commandlinetools-win-16111833_latest.zip"
        );
        assert_eq!(
            a.checksum,
            Checksum::Sha1("57d04f2d75eb8e8fffc5000a987e5de4b5a63e9d".into())
        );
        assert_eq!(a.size, Some(154_957_218));
        assert_eq!(
            r.windows_artifact(GOOGLE_REPOSITORY, "platforms/android-36")
                .unwrap()
                .file_name,
            "platform-36_r02.zip",
            "host-less archive"
        );
        assert!(r
            .windows_artifact(GOOGLE_REPOSITORY, "platforms;android-99")
            .is_err());
        assert!(parse_repository("<a/>").is_err());
        assert!(
            parse_repository("<!DOCTYPE x [<!ENTITY a 'b'>]><x/>").is_err(),
            "DTDs refused"
        );
    }

    /// Reads the real manifests when `UMR_META_DIR` points at downloaded copies.
    #[test]
    #[ignore = "needs UMR_META_DIR with real metadata files"]
    fn real_metadata_files() {
        let dir = std::path::PathBuf::from(std::env::var("UMR_META_DIR").unwrap());
        let rd = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        assert!(parse_adoptium(&rd("adoptium.json"))
            .unwrap()
            .url
            .to_ascii_lowercase()
            .ends_with(".zip"));
        assert_ne!(parse_node_index(&rd("node-index.json"), 24).unwrap(), "");
        parse_flutter(&rd("flutter.json"), None, None).unwrap();
        let repo = parse_repository(&rd("repo.xml")).unwrap();
        repo.windows_artifact(GOOGLE_REPOSITORY, "cmdline-tools;latest")
            .unwrap();
        assert!(repo.package("platforms;android-36").is_some());
        assert!(repo.package("ndk;28.2.13676358").is_some());
        let img = parse_repository(&rd("sysimg.xml")).unwrap();
        assert_eq!(
            img.package("system-images/android-36/google_apis/x86_64")
                .unwrap()
                .licence
                .as_deref(),
            Some("android-sdk-license")
        );
    }
}
