//! TEST-002: 20 random hard kills during installs; after each one the next start must
//! recover to a consistent state, and a final run must finish the install.
//!
//! The parent test starts a slow local server and then, 20 times, starts a child process
//! (this test binary, running [`interruption_child`]) that installs three archive components,
//! and kills it with `TerminateProcess` at a pseudo-random moment (until 20 runs were killed) (during download,
//! extraction or commit). After every kill it runs [`crate::txn::recover`] and checks:
//! - no transaction is left unfinished, the staging folder is empty;
//! - every recorded component's folder has its key file, and every component folder that
//!   exists has a record (nothing half-installed is visible).
//!
//! Finally an uninterrupted run must install everything.

#![cfg(test)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use umr_storage::Db;

use crate::engine::{self, ItemState, Job, JobState, Prepared, RunContext, Step, StepKind};
use crate::http::{Http, Policy};
use crate::layout::Layout;
use crate::testkit::{make_zip, tmp_dir, Route, TestServer};
use crate::verify::{sha256_hex, Checksum};

const SPEC_ENV: &str = "UMR_INTERRUPT_SPEC";

fn http() -> Http {
    Http::new(Policy {
        hosts: vec![],
        allow_loopback_http: true,
        retries: 2,
        backoff: Duration::from_millis(20),
    })
    .unwrap()
}

struct Archive {
    name: &'static str,
    key: &'static str,
    zip: Vec<u8>,
}

/// Three ~1 MB archives (incompressible filler so a kill can land mid-download).
fn archives(dir: &Path) -> Vec<Archive> {
    [
        ("jdk", "jdk-17", r"bin\java.exe"),
        ("node", "node-v24", "node.exe"),
        ("flutter", "flutter", r"bin\flutter.bat"),
    ]
    .into_iter()
    .map(|(name, top, key)| {
        let mut x: u64 = 0x2545_F491_4F6C_DD1D ^ name.len() as u64;
        let filler: Vec<u8> = (0..1_000_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x.to_le_bytes()[3]
            })
            .collect();
        let key_path = format!("{top}/{}", key.replace('\\', "/"));
        let zip = std::fs::read(make_zip(
            &dir.join(format!("{name}.zip")),
            &[(&format!("{top}/data.bin"), &filler), (&key_path, b"MZ")],
        ))
        .unwrap();
        Archive { name, key, zip }
    })
    .collect()
}

/// Served slowly: 64 KB every 30 ms (≈ 0.5 s per archive).
fn routes(archives: &[Archive]) -> HashMap<String, Route> {
    archives
        .iter()
        .map(|a| {
            (
                format!("/{}.zip", a.name),
                Route {
                    throttle: Some((64 * 1024, Duration::from_millis(30))),
                    ..Route::body(&a.zip)
                },
            )
        })
        .collect()
}

fn prepared(base: &str, root: &Path, archives: &[Archive]) -> Prepared {
    let layout = Layout::new(root);
    let steps = archives
        .iter()
        .map(|a| {
            let target = match a.name {
                "jdk" => layout.jdks().join("temurin-17"),
                "node" => layout.node().join("24"),
                _ => layout.flutter().join("3"),
            };
            Step {
                component: a.name.into(),
                title: a.name.into(),
                package: None,
                version: Some("1".into()),
                licences: vec![],
                approx_download_bytes: a.zip.len() as u64,
                kind: StepKind::Archive {
                    component_id: format!("{}@1", a.name),
                    kind: a.name.into(),
                    artifact: crate::catalog::Artifact {
                        url: format!("{base}/{}.zip", a.name),
                        file_name: format!("{}.zip", a.name),
                        checksum: Checksum::Sha256(sha256_hex(&a.zip)),
                        size: Some(a.zip.len() as u64),
                        version: "1".into(),
                    },
                    target: target.display().to_string(),
                    strip_top: true,
                    check: a.key.into(),
                },
            }
        })
        .collect();
    Prepared {
        steps,
        licences: vec![],
        android_sdk: layout.android_sdk().display().to_string(),
    }
}

fn run_install(root: &Path, prepared: &Prepared) -> engine::JobStatus {
    let layout = Layout::new(root);
    let db = Mutex::new(Db::open_in(root).unwrap());
    let http = http();
    let job = Job::new("t", prepared, &layout.logs().join("t.log"));
    let ctx = RunContext {
        http: &http,
        layout: &layout,
        db: &db,
        android_env: HashMap::new(),
        sdk_timeout: Duration::from_secs(5),
    };
    engine::run(&job, prepared, &ctx);
    job.status()
}

/// Child side: installs what the spec file says. Does nothing unless started by the parent.
#[test]
#[ignore = "started by twenty_random_interruptions_always_recover"]
fn interruption_child() {
    let Ok(spec) = std::env::var(SPEC_ENV) else {
        return;
    };
    let (root, prepared): (PathBuf, Prepared) =
        serde_json::from_str(&std::fs::read_to_string(spec).unwrap()).unwrap();
    let _ = run_install(&root, &prepared);
}

fn check_consistent(root: &Path, prepared: &Prepared, round: usize) {
    let layout = Layout::new(root);
    let db = Db::open_in(root).unwrap();
    assert_eq!(
        db.journal_unfinished().unwrap(),
        Vec::new(),
        "round {round}: unfinished transactions after recovery"
    );
    let staging = std::fs::read_dir(layout.staging()).map_or(0, Iterator::count);
    assert_eq!(staging, 0, "round {round}: staging not empty");
    for s in &prepared.steps {
        let StepKind::Archive {
            component_id,
            target,
            check,
            ..
        } = &s.kind
        else {
            continue;
        };
        let target = PathBuf::from(target);
        let recorded = db.component(component_id).unwrap().is_some();
        let complete = target.join(check).is_file();
        assert_eq!(
            recorded, complete,
            "round {round}: {component_id} recorded={recorded} but complete={complete}"
        );
        if target.exists() {
            assert!(
                complete,
                "round {round}: half-installed {} is visible",
                target.display()
            );
        }
    }
}

#[test]
fn twenty_random_interruptions_always_recover() {
    let dir = tmp_dir("interrupt");
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let archives = archives(&dir);
    let server = TestServer::start(routes(&archives));
    let prep = prepared(&server.base, &root, &archives);
    let spec = dir.join("spec.json");
    std::fs::write(&spec, serde_json::to_string(&(&root, &prep)).unwrap()).unwrap();

    let exe = std::env::current_exe().unwrap();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut kills = 0;
    let mut round = 0;
    while kills < 20 && round < 40 {
        round += 1;
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let after = Duration::from_millis(100 + seed % 1400);
        let mut child = std::process::Command::new(&exe)
            .args([
                "--exact",
                "interruptions::interruption_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env(SPEC_ENV, &spec)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while start.elapsed() < after {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap(); // TerminateProcess: no cleanup code runs
            kills += 1;
        }
        let _ = child.wait();
        // "next Core start": recovery, then the invariants
        let db = Mutex::new(Db::open_in(&root).unwrap());
        crate::txn::recover(&Layout::new(&root), &db).unwrap();
        drop(db);
        check_consistent(&root, &prep, round);
        // reset most rounds (every 4th keeps its state, so resuming is tested too)
        if round % 4 != 3 {
            for s in &prep.steps {
                if let StepKind::Archive {
                    component_id,
                    target,
                    ..
                } = &s.kind
                {
                    let _ = std::fs::remove_dir_all(target);
                    Db::open_in(&root)
                        .unwrap()
                        .remove_component(component_id)
                        .unwrap();
                }
            }
        }
    }
    assert_eq!(kills, 20, "20 interrupted installs needed ({round} rounds)");

    let st = run_install(&root, &prep);
    assert_eq!(st.state, JobState::Done, "{st:#?}");
    assert!(st
        .items
        .iter()
        .all(|i| matches!(i.state, ItemState::Done | ItemState::AlreadyInstalled)));
    check_consistent(&root, &prep, 99);
    let _ = std::fs::remove_dir_all(&dir);
}
