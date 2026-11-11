//! Install plan → concrete steps ([`prepare`]) → a job that runs them ([`run`]).
//!
//! Order: JDK, Node, Flutter, Android cmdline-tools (brings Android CLI), then SDK packages
//! with the multi-GB system image last. The first failure stops the job; later steps are
//! marked skipped. Components that cannot be installed automatically yet (Git) are reported
//! and skipped without failing the job.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use umr_resolver::plan::{Action, Plan, PlanItem};
use umr_storage::Db;

use crate::catalog::{self, Artifact, Repository};
use crate::http::Http;
use crate::layout::Layout;
use crate::licence::{self, LicenceText};
use crate::txn::{self, ArchiveInstall, Outcome, SdkPackageInstall, Stage};
use crate::{now_unix, InstallError};

/// Where metadata comes from (official by default; tests point it at a local server).
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub adoptium_api: String,
    pub node_dist: String,
    pub flutter_releases: String,
    /// Folder of `repository2-3.xml` and `sys-img/`.
    pub google_repository: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            adoptium_api: catalog::ADOPTIUM_API.into(),
            node_dist: catalog::NODE_DIST.into(),
            flutter_releases: catalog::FLUTTER_RELEASES.into(),
            google_repository: catalog::GOOGLE_REPOSITORY.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StepKind {
    Archive {
        component_id: String,
        kind: String,
        artifact: Artifact,
        target: String,
        strip_top: bool,
        check: String,
    },
    SdkPackage {
        package: String,
    },
    /// Cannot be installed automatically yet; the reason says what to do.
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub component: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Licence ids this step needs accepted.
    pub licences: Vec<String>,
    pub approx_download_bytes: u64,
    pub kind: StepKind,
}

/// Everything needed to start an install, shown to the user before approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prepared {
    pub steps: Vec<Step>,
    /// Licences that need explicit acceptance, with full text.
    pub licences: Vec<LicenceText>,
    pub android_sdk: String,
}

impl Prepared {
    #[must_use]
    pub fn unaccepted(&self) -> Vec<&LicenceText> {
        self.licences.iter().filter(|l| !l.accepted).collect()
    }
}

fn order(component: &str) -> u8 {
    match component {
        "jdk" => 0,
        "node" => 1,
        "flutter" => 2,
        "git" => 3,
        "android-cmdline-tools" => 4,
        "android-system-image" => 9,
        _ => 5,
    }
}

fn sanitize(v: &str) -> String {
    v.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

struct Repos<'a> {
    http: &'a Http,
    ep: &'a Endpoints,
    main: Option<Repository>,
    images: BTreeMap<String, Repository>,
}

impl Repos<'_> {
    fn main(&mut self) -> Result<&Repository, InstallError> {
        if self.main.is_none() {
            let url = format!(
                "{}/repository2-3.xml",
                self.ep.google_repository.trim_end_matches('/')
            );
            self.main = Some(catalog::parse_repository(
                &self.http.get_text(&url, catalog::MAX_METADATA_BYTES)?,
            )?);
        }
        Ok(self.main.as_ref().unwrap_or_else(|| unreachable!()))
    }

    fn image(&mut self, tag: &str) -> Result<&Repository, InstallError> {
        if !self.images.contains_key(tag) {
            let url = catalog::sysimg_url(&self.ep.google_repository, tag);
            let r =
                catalog::parse_repository(&self.http.get_text(&url, catalog::MAX_METADATA_BYTES)?)?;
            self.images.insert(tag.into(), r);
        }
        Ok(&self.images[tag])
    }

    /// Repository that lists `package` (system images live in their own manifest).
    fn for_package(&mut self, package: &str) -> Result<&Repository, InstallError> {
        if package.starts_with("system-images/") {
            let tag = package.split('/').nth(2).unwrap_or("default").to_string();
            self.image(&tag)
        } else {
            self.main()
        }
    }
}

/// Resolves the plan's install items into steps with exact downloads and licence texts.
/// Reads official metadata (network); downloads nothing large and changes nothing.
#[allow(clippy::too_many_lines)]
pub fn prepare(
    http: &Http,
    ep: &Endpoints,
    layout: &Layout,
    plan: &Plan,
    db: &Mutex<Db>,
) -> Result<Prepared, InstallError> {
    let mut items: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|i| matches!(i.action, Action::Install))
        .collect();
    items.sort_by_key(|i| order(&i.component));
    let mut repos = Repos {
        http,
        ep,
        main: None,
        images: BTreeMap::new(),
    };
    let mut steps = Vec::new();
    let mut licence_ids: Vec<String> = Vec::new();
    for i in items {
        let base = |kind: StepKind, licences: Vec<String>, version: Option<String>| Step {
            component: i.component.clone(),
            title: i.title.clone(),
            package: i.package.clone(),
            version: version.or_else(|| i.version.clone()),
            licences,
            approx_download_bytes: i.approx_download_bytes,
            kind,
        };
        let step = match i.component.as_str() {
            "jdk" => {
                let major: u32 = i.version.as_deref().and_then(|v| v.split('.').next()?.parse().ok()).unwrap_or(17);
                let a = catalog::parse_adoptium(&http.get_text(&catalog::adoptium_url(&ep.adoptium_api, major), catalog::MAX_METADATA_BYTES)?)?;
                let target = layout.jdks().join(format!("temurin-{}", sanitize(&a.version)));
                let v = a.version.clone();
                base(
                    StepKind::Archive {
                        component_id: format!("jdk/temurin@{}", a.version),
                        kind: "jdk".into(),
                        artifact: a,
                        target: target.display().to_string(),
                        strip_top: true,
                        check: r"bin\java.exe".into(),
                    },
                    Vec::new(),
                    Some(v),
                )
            }
            "node" => {
                let major: u64 = i.version.as_deref().and_then(|v| v.split('.').next()?.parse().ok()).unwrap_or(24);
                let index = http.get_text(&format!("{}/index.json", ep.node_dist.trim_end_matches('/')), catalog::MAX_METADATA_BYTES)?;
                let version = catalog::parse_node_index(&index, major)?;
                let sums = http.get_text(&catalog::node_shasums_url(&ep.node_dist, &version), catalog::MAX_METADATA_BYTES)?;
                let a = catalog::node_artifact(&ep.node_dist, &version, &sums)?;
                base(
                    StepKind::Archive {
                        component_id: format!("node@{version}"),
                        kind: "node".into(),
                        artifact: a,
                        target: layout.node().join(&version).display().to_string(),
                        strip_top: true,
                        check: "node.exe".into(),
                    },
                    Vec::new(),
                    Some(version),
                )
            }
            "flutter" => {
                let json = http.get_text(&ep.flutter_releases, catalog::MAX_METADATA_BYTES)?;
                let a = catalog::parse_flutter(&json, i.version.as_deref(), i.min.as_deref())?;
                let v = a.version.clone();
                base(
                    StepKind::Archive {
                        component_id: format!("flutter@{v}"),
                        kind: "flutter".into(),
                        artifact: a,
                        target: layout.flutter().join(sanitize(&v)).display().to_string(),
                        strip_top: true,
                        check: r"bin\flutter.bat".into(),
                    },
                    Vec::new(),
                    Some(v),
                )
            }
            "android-cmdline-tools" => {
                let repo = repos.main()?;
                let a = repo.windows_artifact(&ep.google_repository, "cmdline-tools;latest")?;
                let lic = repo.package("cmdline-tools;latest").and_then(|p| p.licence.clone()).unwrap_or_else(|| "android-sdk-license".into());
                let v = a.version.clone();
                base(
                    StepKind::Archive {
                        component_id: "android/cmdline-tools/latest".into(),
                        kind: "android-sdk-package".into(),
                        artifact: a,
                        target: layout.android_sdk().join("cmdline-tools").join("latest").display().to_string(),
                        strip_top: true,
                        check: r"bin\android.exe".into(),
                    },
                    vec![lic],
                    Some(v),
                )
            }
            c if c.starts_with("android-") => {
                let Some(pkg) = i.package.clone() else {
                    return Err(InstallError::Metadata(format!("{c} has no SDK package name")));
                };
                let repo = repos.for_package(&pkg)?;
                let found = repo.package(&pkg);
                if found.is_none() {
                    return Err(InstallError::Metadata(format!("{pkg} is not in the Android SDK repository")));
                }
                let lic = found.and_then(|p| p.licence.clone()).unwrap_or_else(|| "android-sdk-license".into());
                let v = found.map(|p| p.revision.clone());
                base(StepKind::SdkPackage { package: pkg }, vec![lic], v)
            }
            "git" => base(
                StepKind::Unsupported {
                    reason: "Git is needed by Flutter but cannot be installed automatically yet. Install Git for Windows from https://git-scm.com/download/win, then run the install again.".into(),
                },
                Vec::new(),
                None,
            ),
            other => base(StepKind::Unsupported { reason: format!("{other} cannot be installed automatically yet") }, Vec::new(), None),
        };
        for l in &step.licences {
            if !licence_ids.contains(l) {
                licence_ids.push(l.clone());
            }
        }
        steps.push(step);
    }

    let mut licences = Vec::new();
    for id in licence_ids {
        let text = repos
            .main
            .as_ref()
            .and_then(|r| licence::from_repository(r, &id))
            .or_else(|| {
                repos
                    .images
                    .values()
                    .find_map(|r| licence::from_repository(r, &id))
            });
        let text = match text {
            Some(t) => t,
            None => licence::from_repository(repos.main()?, &id)
                .ok_or_else(|| InstallError::Metadata(format!("licence text '{id}' not found")))?,
        };
        licences.push(text);
    }
    licence::mark_accepted(db, &mut licences)?;
    Ok(Prepared {
        steps,
        licences,
        android_sdk: layout.android_sdk().display().to_string(),
    })
}

// ---- job ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemState {
    Pending,
    Downloading,
    Extracting,
    Committing,
    Installing,
    Done,
    AlreadyInstalled,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStatus {
    pub component: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub state: ItemState,
    pub bytes_done: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobStatus {
    pub id: String,
    pub state: JobState,
    pub items: Vec<ItemStatus>,
    pub started_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Android CLI output.
    pub log: String,
}

/// A running or finished install job, shared between the worker thread and API calls.
#[derive(Debug)]
pub struct Job {
    status: Mutex<JobStatus>,
    cancel: AtomicBool,
}

impl Job {
    #[must_use]
    pub fn new(id: &str, prepared: &Prepared, log: &std::path::Path) -> Self {
        let items = prepared
            .steps
            .iter()
            .map(|s| ItemStatus {
                component: s.component.clone(),
                title: s.title.clone(),
                package: s.package.clone(),
                version: s.version.clone(),
                state: ItemState::Pending,
                bytes_done: 0,
                bytes_total: None,
                message: None,
            })
            .collect();
        Self {
            status: Mutex::new(JobStatus {
                id: id.into(),
                state: JobState::Running,
                items,
                started_unix: now_unix(),
                finished_unix: None,
                message: None,
                log: log.display().to_string(),
            }),
            cancel: AtomicBool::new(false),
        }
    }

    #[must_use]
    pub fn status(&self) -> JobStatus {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.status().state == JobState::Running
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn item(&self, i: usize, f: impl FnOnce(&mut ItemStatus)) {
        self.update(|s| {
            if let Some(it) = s.items.get_mut(i) {
                f(it);
            }
        });
    }
}

/// What [`run`] needs besides the prepared steps.
#[derive(Debug)]
pub struct RunContext<'a> {
    pub http: &'a Http,
    pub layout: &'a Layout,
    pub db: &'a Mutex<Db>,
    /// Isolated child environment for Android CLI (umr-env recipe with the managed SDK).
    pub android_env: HashMap<String, String>,
    pub sdk_timeout: Duration,
}

/// Runs the steps. Blocking; call on a worker thread. Never runs a step before the licence
/// gate has passed.
#[allow(clippy::too_many_lines)]
pub fn run(job: &Job, prepared: &Prepared, ctx: &RunContext<'_>) {
    if let Err(e) = licence::require_accepted(ctx.db, &prepared.licences) {
        job.update(|s| {
            s.state = JobState::Failed;
            s.message = Some(e.to_string());
            s.finished_unix = Some(now_unix());
            for it in &mut s.items {
                it.state = ItemState::Skipped;
            }
        });
        return;
    }
    let log = PathBuf::from(job.status().log);
    let mut failed: Option<InstallError> = None;
    for (i, step) in prepared.steps.iter().enumerate() {
        if let Some(e) = &failed {
            let msg = if matches!(e, InstallError::Cancelled) {
                "cancelled"
            } else {
                "skipped after an earlier failure"
            };
            job.item(i, |it| {
                it.state = ItemState::Skipped;
                it.message = Some(msg.into());
            });
            continue;
        }
        let mut on = |stage: Stage, done: u64, total: Option<u64>| {
            job.item(i, |it| {
                it.state = match stage {
                    Stage::Downloading => ItemState::Downloading,
                    Stage::Extracting => ItemState::Extracting,
                    Stage::Committing => ItemState::Committing,
                    Stage::Installing => ItemState::Installing,
                };
                if stage == Stage::Downloading {
                    it.bytes_done = done;
                    it.bytes_total = total;
                }
            });
        };
        let result = match &step.kind {
            StepKind::Archive {
                component_id,
                kind,
                artifact,
                target,
                strip_top,
                check,
            } => txn::install_archive(
                ctx.http,
                ctx.layout,
                ctx.db,
                &ArchiveInstall {
                    component_id: component_id.clone(),
                    kind: kind.clone(),
                    artifact: artifact.clone(),
                    target: PathBuf::from(target),
                    strip_top: *strip_top,
                    check: PathBuf::from(check),
                },
                &mut on,
                &job.cancel,
            ),
            StepKind::SdkPackage { package } => {
                let sdk = ctx.layout.android_sdk();
                txn::install_sdk_package(
                    ctx.db,
                    &SdkPackageInstall {
                        package: package.clone(),
                        android_exe: sdk
                            .join("cmdline-tools")
                            .join("latest")
                            .join("bin")
                            .join("android.exe"),
                        sdk_root: sdk,
                        env: ctx.android_env.clone(),
                        log: log.clone(),
                        timeout: ctx.sdk_timeout,
                    },
                    &mut on,
                    &job.cancel,
                )
            }
            StepKind::Unsupported { reason } => {
                job.item(i, |it| {
                    it.state = ItemState::Skipped;
                    it.message = Some(reason.clone());
                });
                continue;
            }
        };
        match result {
            Ok(o) => job.item(i, |it| {
                it.state = if o == Outcome::AlreadyInstalled {
                    ItemState::AlreadyInstalled
                } else {
                    ItemState::Done
                };
                if let Some(t) = it.bytes_total {
                    it.bytes_done = t;
                }
            }),
            Err(e) => {
                let msg = e.to_string();
                job.item(i, |it| {
                    it.state = if matches!(e, InstallError::Cancelled) {
                        ItemState::Skipped
                    } else {
                        ItemState::Failed
                    };
                    it.message = Some(msg);
                });
                failed = Some(e);
            }
        }
    }
    job.update(|s| {
        s.finished_unix = Some(now_unix());
        match failed {
            None => s.state = JobState::Done,
            Some(InstallError::Cancelled) => {
                s.state = JobState::Cancelled;
                s.message =
                    Some("cancelled; downloads are kept and resume on the next install".into());
            }
            Some(e) => {
                s.state = JobState::Failed;
                s.message = Some(e.to_string());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use super::*;
    use crate::catalog::tests::REPO;
    use crate::http::Policy;
    use crate::testkit::{make_zip, tmp_dir, Route, TestServer};
    use crate::verify::sha256_hex;

    fn item(component: &str, package: Option<&str>, version: Option<&str>) -> PlanItem {
        PlanItem {
            component: component.into(),
            title: component.into(),
            package: package.map(String::from),
            version: version.map(String::from),
            min: None,
            action: Action::Install,
            approx_download_bytes: 1,
            approx_disk_bytes: 1,
            licence: None,
            reason: String::new(),
            sources: vec![],
        }
    }

    fn plan(items: Vec<PlanItem>) -> Plan {
        Plan {
            project: "p".into(),
            profile: "android-native".into(),
            pack_version: "x".into(),
            items,
            licences: vec![],
            admin_actions: vec![],
            conflicts: vec![],
            notes: vec![],
            total_download_bytes: 0,
            total_disk_bytes: 0,
            disk_free_bytes: 0,
            disk_ok: true,
            android_sdk: None,
        }
    }

    struct Fx {
        layout: Layout,
        db: Mutex<Db>,
        http: Http,
        ep: Endpoints,
        _servers: (TestServer, TestServer),
    }

    /// Local stand-ins for Adoptium, nodejs.org and the Google repository.
    fn fx(tag: &str) -> Fx {
        let dir = tmp_dir(tag);
        let layout = Layout::new(&dir.join("root"));
        std::fs::create_dir_all(layout.root()).unwrap();
        let db = Mutex::new(Db::open_in(layout.root()).unwrap());
        let jdk = std::fs::read(make_zip(
            &dir.join("j.zip"),
            &[("jdk-17.0.21+9/bin/java.exe", b"MZ".as_slice())],
        ))
        .unwrap();
        let node = std::fs::read(make_zip(
            &dir.join("n.zip"),
            &[("node-v24.15.0-win-x64/node.exe", b"MZ".as_slice())],
        ))
        .unwrap();
        let clt = std::fs::read(make_zip(
            &dir.join("c.zip"),
            &[("cmdline-tools/bin/android.exe", b"MZ".as_slice())],
        ))
        .unwrap();
        let mut routes: Map<String, Route> = Map::new();
        // Downloads and Google/Node metadata; Adoptium (which needs this server's URL) below.
        let s = TestServer::start({
            routes.insert("/jdk.zip".into(), Route::body(&jdk));
            routes.insert(
                "/dist/v24.15.0/node-v24.15.0-win-x64.zip".into(),
                Route::body(&node),
            );
            routes.insert(
                "/dist/v24.15.0/SHASUMS256.txt".into(),
                Route::body(
                    format!("{}  node-v24.15.0-win-x64.zip\n", sha256_hex(&node)).as_bytes(),
                ),
            );
            routes.insert(
                "/dist/index.json".into(),
                Route::body(br#"[{"version":"v24.15.0","files":["win-x64-zip"]}]"#),
            );
            let clt_sha1 = crate::verify::hash_file(
                &dir.join("c.zip"),
                &crate::verify::Checksum::Sha1(String::new()),
            )
            .unwrap();
            let repo = REPO
                .replace("57D04F2D75EB8E8FFFC5000A987E5DE4B5A63E9D", &clt_sha1)
                .replace("<size>154957218</size>", "");
            routes.insert(
                "/android/repository2-3.xml".into(),
                Route::body(repo.as_bytes()),
            );
            routes.insert(
                "/android/commandlinetools-win-16111833_latest.zip".into(),
                Route::body(&clt),
            );
            routes
        });
        // Adoptium answer must contain this server's absolute URL.
        let adoptium = format!(
            r#"[{{"binary":{{"os":"windows","architecture":"x64","image_type":"jdk","package":{{"name":"jdk.zip","link":"{}/jdk.zip","checksum":"{}","size":{}}}}},"version":{{"openjdk_version":"17.0.21+9"}}}}]"#,
            s.base,
            sha256_hex(&jdk),
            jdk.len()
        );
        let s2 = TestServer::start(Map::from([(
            "/v3/assets/latest/17/hotspot".to_string(),
            Route::body(adoptium.as_bytes()),
        )]));
        let http = Http::new(Policy {
            hosts: vec![],
            allow_loopback_http: true,
            retries: 1,
            backoff: Duration::from_millis(5),
        })
        .unwrap();
        let ep = Endpoints {
            adoptium_api: s2.base.clone(),
            node_dist: s.url("/dist"),
            flutter_releases: s.url("/flutter.json"),
            google_repository: s.url("/android"),
        };
        Fx {
            layout,
            db,
            http,
            ep,
            _servers: (s, s2),
        }
    }

    #[test]
    fn prepare_orders_steps_and_collects_licences() {
        let f = fx("eng-prep");
        let p = plan(vec![
            item("android-platform", Some("platforms/android-36"), Some("36")),
            item("android-cmdline-tools", Some("cmdline-tools/latest"), None),
            item("git", None, None),
            item("node", None, Some("24")),
            item("jdk", None, Some("17")),
        ]);
        let prep = prepare(&f.http, &f.ep, &f.layout, &p, &f.db).unwrap();
        let order: Vec<&str> = prep.steps.iter().map(|s| s.component.as_str()).collect();
        assert_eq!(
            order,
            [
                "jdk",
                "node",
                "git",
                "android-cmdline-tools",
                "android-platform"
            ]
        );
        assert_eq!(prep.steps[0].version.as_deref(), Some("17.0.21+9"));
        assert_eq!(prep.steps[1].version.as_deref(), Some("24.15.0"));
        assert!(matches!(prep.steps[2].kind, StepKind::Unsupported { .. }));
        assert_eq!(
            prep.steps[4].kind,
            StepKind::SdkPackage {
                package: "platforms/android-36".into()
            }
        );
        assert_eq!(prep.licences.len(), 1);
        assert_eq!(prep.licences[0].id, "android-sdk-license");
        assert!(!prep.licences[0].accepted);
        assert!(prep.licences[0].text.contains("License Agreement"));
    }

    #[test]
    fn run_refuses_without_licence_then_installs() {
        let f = fx("eng-run");
        let p = plan(vec![
            item("jdk", None, Some("17")),
            item("node", None, Some("24")),
            item("android-cmdline-tools", Some("cmdline-tools/latest"), None),
        ]);
        let prep = prepare(&f.http, &f.ep, &f.layout, &p, &f.db).unwrap();
        let ctx = RunContext {
            http: &f.http,
            layout: &f.layout,
            db: &f.db,
            android_env: HashMap::new(),
            sdk_timeout: Duration::from_secs(5),
        };

        let job = Job::new("j1", &prep, &f.layout.logs().join("j1.log"));
        run(&job, &prep, &ctx);
        let st = job.status();
        assert_eq!(st.state, JobState::Failed);
        assert!(st.message.unwrap().contains("android-sdk-license"));
        assert!(
            !f.layout.jdks().exists(),
            "nothing installed before acceptance"
        );

        let l = &prep.licences[0];
        f.db.lock()
            .unwrap()
            .accept_licence(&l.id, &l.sha256, Some("test"), 1)
            .unwrap();
        let job = Job::new("j2", &prep, &f.layout.logs().join("j2.log"));
        run(&job, &prep, &ctx);
        let st = job.status();
        assert_eq!(st.state, JobState::Done, "{st:#?}");
        assert!(st.items.iter().all(|i| i.state == ItemState::Done));
        assert!(f
            .layout
            .jdks()
            .join("temurin-17.0.21_9")
            .join("bin")
            .join("java.exe")
            .is_file());
        assert!(f.layout.node().join("24.15.0").join("node.exe").is_file());
        assert!(f
            .layout
            .android_sdk()
            .join("cmdline-tools")
            .join("latest")
            .join("bin")
            .join("android.exe")
            .is_file());
        // the environment scan finds the managed JDK and Node where the installer put them
        let scan = umr_env::scan(&umr_env::context::ScanContext::for_test(
            f.layout.root().to_path_buf(),
            &[],
        ));
        assert!(
            scan.jdks
                .iter()
                .any(|j| j.home.contains("temurin-17.0.21_9")),
            "{:?}",
            scan.jdks
        );
        assert!(scan
            .tool("node")
            .is_some_and(|t| t.path.contains("24.15.0")));
    }

    #[test]
    fn failure_skips_the_rest_and_cancel_stops() {
        let f = fx("eng-fail");
        let p = plan(vec![
            item("jdk", None, Some("17")),
            item("node", None, Some("24")),
        ]);
        let mut prep = prepare(&f.http, &f.ep, &f.layout, &p, &f.db).unwrap();
        if let StepKind::Archive { artifact, .. } = &mut prep.steps[0].kind {
            artifact.checksum = crate::verify::Checksum::Sha256("00".repeat(32));
        }
        let ctx = RunContext {
            http: &f.http,
            layout: &f.layout,
            db: &f.db,
            android_env: HashMap::new(),
            sdk_timeout: Duration::from_secs(5),
        };
        let job = Job::new("j", &prep, &f.layout.logs().join("j.log"));
        run(&job, &prep, &ctx);
        let st = job.status();
        assert_eq!(st.state, JobState::Failed);
        assert_eq!(
            (st.items[0].state, st.items[1].state),
            (ItemState::Failed, ItemState::Skipped)
        );

        let job = Job::new("c", &prep, &f.layout.logs().join("c.log"));
        job.cancel();
        run(&job, &prep, &ctx);
        assert_eq!(job.status().state, JobState::Cancelled);
    }
}
