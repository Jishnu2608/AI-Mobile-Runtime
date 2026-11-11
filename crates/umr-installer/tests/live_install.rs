//! Live check against the real official servers (downloads ~230 MB). Not run by default:
//! `cargo test -p umr-installer --test live_install -- --ignored --nocapture`

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use umr_installer::engine::{self, Endpoints, ItemState, Job, JobState, RunContext};
use umr_installer::http::{Http, Policy};
use umr_installer::layout::Layout;
use umr_resolver::plan::{Action, Plan, PlanItem};

fn item(component: &str, version: &str) -> PlanItem {
    PlanItem {
        component: component.into(),
        title: component.into(),
        package: None,
        version: Some(version.into()),
        min: None,
        action: Action::Install,
        approx_download_bytes: 0,
        approx_disk_bytes: 0,
        licence: None,
        reason: String::new(),
        sources: vec![],
    }
}

#[test]
#[ignore = "downloads the real Temurin JDK 17 and Node.js 24 (~230 MB)"]
fn real_jdk_and_node_install_into_a_temp_root() {
    let root = std::env::temp_dir().join(format!("umr-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let layout = Layout::new(&root);
    let db = Mutex::new(umr_storage::Db::open_in(&root).unwrap());
    let http = Http::new(Policy::official()).unwrap();
    let plan = Plan {
        project: "live".into(),
        profile: "android-native".into(),
        pack_version: "x".into(),
        items: vec![item("jdk", "17"), item("node", "24")],
        licences: vec![],
        admin_actions: vec![],
        conflicts: vec![],
        notes: vec![],
        total_download_bytes: 0,
        total_disk_bytes: 0,
        disk_free_bytes: 0,
        disk_ok: true,
        android_sdk: None,
    };
    let prepared = engine::prepare(&http, &Endpoints::default(), &layout, &plan, &db).unwrap();
    assert!(
        prepared.licences.is_empty(),
        "Temurin and Node need no acceptance step"
    );
    let job = Job::new("live", &prepared, &layout.logs().join("live.log"));
    let ctx = RunContext {
        http: &http,
        layout: &layout,
        db: &db,
        android_env: HashMap::new(),
        sdk_timeout: Duration::from_secs(60),
    };
    let t = std::time::Instant::now();
    engine::run(&job, &prepared, &ctx);
    let st = job.status();
    println!("{st:#?}\ntook {:?}", t.elapsed());
    assert_eq!(st.state, JobState::Done);
    assert!(st.items.iter().all(|i| i.state == ItemState::Done));
    // the installed tools run
    let java = std::fs::read_dir(layout.jdks())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("bin")
        .join("java.exe");
    let out = std::process::Command::new(&java)
        .arg("-version")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    println!("{}", String::from_utf8_lossy(&out.stderr));
    let node = std::fs::read_dir(layout.node())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("node.exe");
    let out = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    println!("node {}", String::from_utf8_lossy(&out.stdout));
    let _ = std::fs::remove_dir_all(&root);
}
