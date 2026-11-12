//! End-to-end tests of the local API: a real `umr-core` process, a real named pipe,
//! and the `umr-api` client.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use umr_api::client::{Client, ClientError};
use umr_api::methods::{self, CoreInfo, CreateClientResult, DoctorReport, ShutdownAccepted};
use umr_api::rpc::codes;
use umr_core::paths::CorePaths;

const BIN: &str = env!("CARGO_BIN_EXE_umr-core");

/// Opens the raw pipe, retrying on `ERROR_PIPE_BUSY` (231) like the real client does:
/// right after a connection the server briefly has no free instance.
async fn open_raw(pipe: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
    let start = Instant::now();
    loop {
        match ClientOptions::new().open(pipe) {
            Ok(c) => return c,
            Err(e) if e.raw_os_error() == Some(231) && start.elapsed() < Duration::from_secs(5) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(e) => panic!("cannot open pipe: {e}"),
        }
    }
}

struct Fixture {
    root: PathBuf,
    child: Option<Child>,
    pipe: String,
}

impl Fixture {
    fn start(tag: &str) -> Self {
        Self::start_with(tag, |_| {})
    }

    /// Like `start`, but lets the test put files into the data root before the Core starts.
    fn start_with(tag: &str, seed: impl FnOnce(&std::path::Path)) -> Self {
        let root = std::env::temp_dir().join(format!("umr-api-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        seed(&root);
        let pipe = CorePaths::with_root(&root).pipe_name().unwrap();
        let child = Command::new(BIN)
            .arg("run")
            .env("UMR_HOME", &root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            root,
            child: Some(child),
            pipe,
        }
    }

    fn admin_token(&self) -> String {
        std::fs::read_to_string(self.root.join("run").join("admin.token"))
            .expect("admin token file")
    }

    /// Connected and logged in with the admin token.
    async fn client(&self) -> Client {
        let mut c = self.client_unauthenticated().await;
        c.login(&self.admin_token()).await.expect("admin login");
        c
    }

    async fn client_unauthenticated(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.pipe).await {
                Ok(c) => return c,
                Err(ClientError::NotRunning) if start.elapsed() < Duration::from_secs(10) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => panic!("cannot connect: {e}"),
            }
        }
    }

    fn wait_exit(&mut self) -> Option<i32> {
        let child = self.child.as_mut()?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Some(status) = child.try_wait().unwrap() {
                self.child = None;
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn ping_info_and_errors_over_the_pipe() {
    let f = Fixture::start("basic");
    let mut c = f.client().await;

    assert_eq!(
        c.call_raw(methods::PING, Value::Null).await.unwrap(),
        json!({"pong": true})
    );

    let info: CoreInfo = c.call(methods::INFO, Value::Null).await.unwrap();
    assert_eq!(info.pid, f.child.as_ref().unwrap().id());
    assert_eq!(info.api_version, umr_api::API_VERSION);
    assert_eq!(PathBuf::from(&info.data_root), f.root);

    match c.call_raw("no.such.method", Value::Null).await {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::METHOD_NOT_FOUND),
        other => panic!("expected method-not-found, got {other:?}"),
    }
    // the connection stays usable after an error
    assert!(c.call_raw(methods::PING, Value::Null).await.is_ok());
}

#[tokio::test]
async fn several_clients_at_once() {
    let f = Fixture::start("multi");
    let mut clients = Vec::new();
    for _ in 0..5 {
        clients.push(f.client().await);
    }
    for c in &mut clients {
        assert!(c.call_raw(methods::PING, Value::Null).await.is_ok());
    }
}

#[tokio::test]
async fn malformed_and_oversized_messages_are_rejected() {
    let f = Fixture::start("bad-input");
    drop(f.client_unauthenticated().await); // wait until the Core is up

    let mut raw = open_raw(&f.pipe).await;
    raw.write_all(b"{this is not json\n").await.unwrap();
    let mut buf = vec![0u8; 512];
    let n = raw.read(&mut buf).await.unwrap();
    let resp: Value = serde_json::from_slice(&buf[..n]).unwrap();
    assert_eq!(resp["error"]["code"], json!(codes::PARSE_ERROR));
    assert_eq!(resp["id"], Value::Null);

    let mut big = open_raw(&f.pipe).await;
    let payload = vec![b'x'; umr_api::MAX_MESSAGE_BYTES + 10];
    let _ = big.write_all(&payload).await; // server may close before reading everything
    let n = big.read(&mut buf).await.unwrap_or(0);
    let resp: Value = serde_json::from_slice(&buf[..n]).unwrap();
    assert_eq!(resp["error"]["code"], json!(codes::MESSAGE_TOO_LARGE));

    // the Core is still healthy
    let mut c = f.client().await;
    assert!(c.call_raw(methods::PING, Value::Null).await.is_ok());
}

#[tokio::test]
async fn shutdown_request_stops_the_core_cleanly() {
    let mut f = Fixture::start("shutdown");
    let mut c = f.client().await;
    let ack: ShutdownAccepted = c.call(methods::SHUTDOWN, Value::Null).await.unwrap();
    assert!(ack.stopping);
    drop(c);
    assert_eq!(f.wait_exit(), Some(0), "Core must exit with code 0");
    assert!(
        !f.root.join("run").join("core.pid").exists(),
        "pid file removed on clean stop"
    );
    assert!(matches!(
        Client::connect(&f.pipe).await,
        Err(ClientError::NotRunning)
    ));
}

#[tokio::test]
async fn client_reports_not_running_when_no_core() {
    let root = std::env::temp_dir().join(format!("umr-api-none-{}", std::process::id()));
    let pipe = CorePaths::with_root(&root).pipe_name().unwrap();
    assert!(matches!(
        Client::connect(&pipe).await,
        Err(ClientError::NotRunning)
    ));
}

#[tokio::test]
async fn unauthenticated_and_wrong_token_calls_are_rejected() {
    let f = Fixture::start("auth");
    let mut c = f.client_unauthenticated().await;
    assert!(
        c.call_raw(methods::PING, Value::Null).await.is_ok(),
        "ping needs no login"
    );
    match c.call_raw(methods::INFO, Value::Null).await {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::UNAUTHENTICATED),
        other => panic!("expected unauthenticated, got {other:?}"),
    }
    match c.login("umr_0000").await {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::UNAUTHENTICATED),
        other => panic!("expected invalid token, got {other:?}"),
    }
    c.login(&f.admin_token()).await.unwrap();
    let report: DoctorReport = c.call(methods::DOCTOR, Value::Null).await.unwrap();
    assert!(report.checks.iter().any(|ch| ch.id == "os.windows11"));
}

#[tokio::test]
async fn env_scan_returns_report_and_is_stored() {
    let f = Fixture::start("env-scan");
    let mut c = f.client().await;
    let report: umr_env::EnvReport = c.call(methods::ENV_SCAN, Value::Null).await.unwrap();
    assert!(report.host.windows_build > 10_000);
    assert_eq!(report.readiness.len(), 4);
    let db = umr_storage::Db::open_in(&f.root).unwrap();
    assert_eq!(db.env_scan_count().unwrap(), 1, "scan must be stored");
    // observe scope is enough; no login is not
    let mut anon = f.client_unauthenticated().await;
    match anon.call_raw(methods::ENV_SCAN, Value::Null).await {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::UNAUTHENTICATED),
        other => panic!("expected unauthenticated, got {other:?}"),
    }
    // the connection keeps working after the slow call
    assert!(c.call_raw(methods::PING, Value::Null).await.is_ok());
}

#[tokio::test]
async fn project_plan_reads_project_and_does_not_change_it() {
    let f = Fixture::start("project-plan");
    let proj = f.root.join("proj");
    std::fs::create_dir_all(proj.join("gradle").join("wrapper")).unwrap();
    std::fs::create_dir_all(proj.join("app")).unwrap();
    std::fs::write(
        proj.join("settings.gradle.kts"),
        "plugins { id(\"com.android.application\") version \"8.7.3\" apply false }\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("gradle")
            .join("wrapper")
            .join("gradle-wrapper.properties"),
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-8.4-bin.zip\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("app").join("build.gradle.kts"),
        "android {\n  compileSdk = 35\n}\n",
    )
    .unwrap();
    let before = std::fs::read(proj.join("settings.gradle.kts")).unwrap();

    let mut c = f.client().await;
    let params = serde_json::json!({ "path": proj.display().to_string() });
    let v: Value = c.call(methods::PROJECT_PLAN, params).await.unwrap();
    assert_eq!(v["project"]["framework"], "android-native");
    assert_eq!(v["project"]["android"]["agp"], "8.7.3");
    let conflicts = v["plan"]["conflicts"].as_array().unwrap();
    assert!(
        conflicts
            .iter()
            .any(|c| c["id"] == "gradle-too-old-for-agp"),
        "{conflicts:?}"
    );
    let items = v["plan"]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["package"] == "platforms/android-35"));
    assert!(items.iter().any(|i| i["component"] == "jdk"));
    assert_eq!(
        std::fs::read(proj.join("settings.gradle.kts")).unwrap(),
        before,
        "project untouched"
    );
    let db = umr_storage::Db::open_in(&f.root).unwrap();
    assert_eq!(
        db.env_scan_count().unwrap(),
        1,
        "no stored scan: plan scans first"
    );
    drop(db);

    // relative / missing paths are rejected
    match c
        .call_raw(
            methods::PROJECT_PLAN,
            serde_json::json!({ "path": "relative" }),
        )
        .await
    {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::INVALID_PARAMS),
        other => panic!("expected invalid params, got {other:?}"),
    }
}

#[tokio::test]
async fn install_api_guards_without_network() {
    let f = Fixture::start("install-guards");
    let mut c = f.client().await;
    let rpc_code = |r: Result<Value, ClientError>| match r {
        Err(ClientError::Rpc(e)) => e.code,
        other => panic!("expected an RPC error, got {other:?}"),
    };
    // a licence text this Core never showed cannot be accepted
    let r = c
        .call_raw(
            methods::INSTALL_ACCEPT_LICENCE,
            serde_json::json!({ "id": "android-sdk-license", "sha256": "00" }),
        )
        .await;
    assert_eq!(rpc_code(r), codes::INVALID_PARAMS);
    let db = umr_storage::Db::open_in(&f.root).unwrap();
    assert!(!db.licence_accepted("android-sdk-license", "00").unwrap());
    drop(db);
    // no job yet
    assert_eq!(
        rpc_code(c.call_raw(methods::INSTALL_STATUS, Value::Null).await),
        codes::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(
            c.call_raw(methods::INSTALL_CANCEL, serde_json::json!({}))
                .await
        ),
        codes::INVALID_PARAMS
    );
    // a bad project path is rejected before any network use
    let r = c
        .call_raw(
            methods::INSTALL_START,
            serde_json::json!({ "path": "relative" }),
        )
        .await;
    assert_eq!(rpc_code(r), codes::INVALID_PARAMS);
    // not logged in: refused
    let mut anon = f.client_unauthenticated().await;
    let r = anon
        .call_raw(
            methods::INSTALL_PREPARE,
            serde_json::json!({ "path": "C:\\" }),
        )
        .await;
    assert_eq!(rpc_code(r), codes::UNAUTHENTICATED);
}

#[tokio::test]
async fn components_and_admin_api_guards() {
    let f = Fixture::start("components");
    let mut c = f.client().await;
    let rpc_code = |r: Result<Value, ClientError>| match r {
        Err(ClientError::Rpc(e)) => e.code,
        other => panic!("expected an RPC error, got {other:?}"),
    };
    let v = c
        .call_raw(methods::COMPONENTS_LIST, Value::Null)
        .await
        .unwrap();
    assert_eq!(v["components"], serde_json::json!([]));
    assert!(v["disk"]["total_bytes"].is_u64());
    let r = c
        .call_raw(
            methods::COMPONENTS_REMOVE,
            serde_json::json!({ "id": "jdk/nope" }),
        )
        .await;
    assert_eq!(rpc_code(r), codes::INVALID_PARAMS);
    let v = c
        .call_raw(methods::COMPONENTS_REPAIR, Value::Null)
        .await
        .unwrap();
    assert_eq!(v["removed"], serde_json::json!([]));
    let v = c
        .call_raw(methods::COMPONENTS_CLEAN, Value::Null)
        .await
        .unwrap();
    assert_eq!(v["freed_bytes"], 0);
    // only the fixed action names are accepted; nothing is elevated for anything else
    for bad in ["cmd", "enable-whpx; calc", ""] {
        let r = c
            .call_raw(methods::ADMIN_RUN, serde_json::json!({ "action": bad }))
            .await;
        assert_eq!(rpc_code(r), codes::INVALID_PARAMS, "{bad}");
    }
    let db = umr_storage::Db::open_in(&f.root).unwrap();
    assert_eq!(db.audit_count("admin.run").unwrap(), 0);
}

#[tokio::test]
async fn separate_toolchains_folder_is_created_private_and_reported() {
    let tc = std::env::temp_dir().join(format!("umr-separate-tc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tc);
    let tc_for_seed = tc.clone();
    let f = Fixture::start_with("toolchains", move |root| {
        std::fs::write(
            root.join("config.toml"),
            format!("[storage]\ntoolchains_dir = '{}'\n", tc_for_seed.display()),
        )
        .unwrap();
    });
    let mut c = f.client().await;
    let info: Value = c.call_raw(methods::INFO, Value::Null).await.unwrap();
    assert_eq!(info["toolchains_root"], tc.display().to_string());
    assert_ne!(info["data_root"], info["toolchains_root"]);
    assert!(tc.is_dir(), "created at start");
    let sddl = umr_platform::secure_file::dir_dacl_sddl(&tc).unwrap();
    assert!(sddl.starts_with("D:P") && !sddl.contains(";AU)"), "{sddl}");
    // the doctor checks both folders
    let doc: Value = c.call_raw(methods::DOCTOR, Value::Null).await.unwrap();
    let ids: Vec<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"data.toolchains") && ids.contains(&"data.redirection"),
        "{ids:?}"
    );
    drop(c);
    drop(f);
    let _ = std::fs::remove_dir_all(&tc);
}

#[tokio::test]
async fn device_list_without_platform_tools_says_so() {
    let f = Fixture::start("device-list");
    let mut c = f.client().await;
    match c.call_raw(methods::DEVICE_LIST, Value::Null).await {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, codes::UNAVAILABLE);
            assert!(e.message.contains("platform-tools"), "{}", e.message);
        }
        other => panic!("expected 'not installed', got {other:?}"),
    }
}

#[tokio::test]
async fn sessions_and_device_interaction_reject_bad_requests() {
    let f = Fixture::start_with("device-session", |root| {
        let img = root
            .join("android-sdk")
            .join("system-images")
            .join("android-36")
            .join("google_apis")
            .join("x86_64");
        std::fs::create_dir_all(&img).unwrap();
        std::fs::write(img.join("system.img"), b"x").unwrap();
    });
    let mut c = f.client().await;
    // sessions: none yet; nothing to stop
    let st = c
        .call_raw(methods::SESSION_STATUS, Value::Null)
        .await
        .unwrap();
    assert!(st["session"].is_null());
    assert_eq!(st["processes"], serde_json::json!([]));
    let stop = c
        .call_raw(methods::SESSION_STOP, Value::Null)
        .await
        .unwrap();
    assert!(stop["session"].is_null());
    // invalid input is rejected before any device is needed
    let bad_pkg = c
        .call_raw(
            methods::SESSION_START,
            serde_json::json!({ "profile": "standard", "package": "x;reboot" }),
        )
        .await;
    assert!(matches!(bad_pkg, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let bad_launch = c
        .call_raw(
            methods::APP_LAUNCH,
            serde_json::json!({ "package": "nodot" }),
        )
        .await;
    assert!(matches!(bad_launch, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let bad_posture = c
        .call_raw(
            methods::DEVICE_POSTURE,
            serde_json::json!({ "posture": "sideways" }),
        )
        .await;
    assert!(matches!(bad_posture, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let bad_action = c
        .call_raw(
            methods::DEVICE_INPUT,
            serde_json::json!({ "action": "shake" }),
        )
        .await;
    assert!(matches!(bad_action, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    // no adb in this fixture: device commands say so
    let shot = c.call_raw(methods::DEVICE_SCREENSHOT, Value::Null).await;
    assert!(matches!(shot, Err(ClientError::Rpc(e)) if e.code == codes::UNAVAILABLE));
    // system image present, emulator missing: the session stops in env_blocked (journaled)
    let blocked = c
        .call_raw(
            methods::SESSION_START,
            serde_json::json!({ "profile": "standard" }),
        )
        .await;
    assert!(matches!(blocked, Err(ClientError::Rpc(e)) if e.code == codes::UNAVAILABLE));
    let st = c
        .call_raw(methods::SESSION_STATUS, Value::Null)
        .await
        .unwrap();
    assert_eq!(st["session"]["state"], "env_blocked");
    assert_eq!(
        st["events"].as_array().unwrap().len(),
        2,
        "start + env_missing"
    );
}

#[tokio::test]
async fn device_catalogue_and_lazy_creation() {
    let f = Fixture::start_with("device-catalogue", |root| {
        // a minimal "installed" system image in the toolchains folder (= home here)
        let img = root
            .join("android-sdk")
            .join("system-images")
            .join("android-36")
            .join("google_apis")
            .join("x86_64");
        std::fs::create_dir_all(&img).unwrap();
        std::fs::write(img.join("system.img"), b"x").unwrap();
    });
    let mut c = f.client().await;
    let v = c
        .call_raw(methods::DEVICE_PROFILES, Value::Null)
        .await
        .unwrap();
    assert_eq!(v["api"], "36");
    let ids: Vec<&str> = v["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["profile"]["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        ["compact", "standard", "large", "fold", "flip", "tablet"]
    );
    assert!(
        v["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["avd"]["exists"] == false),
        "nothing created up front"
    );

    let made = c
        .call_raw(
            methods::DEVICE_CREATE,
            serde_json::json!({ "profile": "fold" }),
        )
        .await
        .unwrap();
    assert_eq!(made["name"], "umr_fold_api36");
    let cfg = std::fs::read_to_string(
        std::path::Path::new(made["dir"].as_str().unwrap()).join("config.ini"),
    )
    .unwrap();
    assert!(cfg.contains("hw.sensor.hinge=yes") && cfg.contains("hw.lcd.width=1768"));
    assert!(
        made["dir"]
            .as_str()
            .unwrap()
            .starts_with(f.root.to_str().unwrap()),
        "inside the managed folder"
    );

    let bad = c
        .call_raw(
            methods::DEVICE_CREATE,
            serde_json::json!({ "profile": "galaxy" }),
        )
        .await;
    assert!(matches!(bad, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let no_img = c
        .call_raw(
            methods::DEVICE_CREATE,
            serde_json::json!({ "profile": "standard", "api": "35" }),
        )
        .await;
    assert!(matches!(no_img, Err(ClientError::Rpc(e)) if e.code == codes::UNAVAILABLE));

    // boot: bad GPU mode is rejected; without an installed emulator nothing starts
    let gpu = c
        .call_raw(
            methods::DEVICE_BOOT,
            serde_json::json!({ "profile": "standard", "gpu": "warp" }),
        )
        .await;
    assert!(matches!(gpu, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let no_adb = c
        .call_raw(
            methods::DEVICE_BOOT,
            serde_json::json!({ "profile": "standard" }),
        )
        .await;
    assert!(matches!(no_adb, Err(ClientError::Rpc(e)) if e.code == codes::UNAVAILABLE));
    let stop = c
        .call_raw(
            methods::DEVICE_STOP,
            serde_json::json!({ "profile": "standard" }),
        )
        .await
        .unwrap();
    assert_eq!(
        stop,
        serde_json::json!({ "avd": "umr_standard_api36", "stopped": false })
    );
}

#[tokio::test]
async fn interrupted_install_is_rolled_back_when_the_core_starts() {
    let f = Fixture::start_with("install-recover", |root| {
        let db = umr_storage::Db::open_in(root).unwrap();
        let staging = root.join("staging").join("t1");
        std::fs::create_dir_all(staging.join("bin")).unwrap();
        db.journal_begin(&umr_storage::install::JournalEntry {
            id: "t1".into(),
            component: "node@24.15.0".into(),
            target: root.join("node").join("24.15.0").display().to_string(),
            staging: Some(staging.display().to_string()),
            state: umr_storage::install::TxnState::Extracted,
            error: None,
            started_unix: 1,
            updated_unix: 1,
        })
        .unwrap();
    });
    let _c = f.client().await; // the Core is up: recovery ran before the API opened
    let db = umr_storage::Db::open_in(&f.root).unwrap();
    let e = db.journal_entry("t1").unwrap().unwrap();
    assert_eq!(e.state, umr_storage::install::TxnState::RolledBack);
    assert!(!f.root.join("staging").join("t1").exists());
    assert!(!f.root.join("node").join("24.15.0").exists());
}

#[tokio::test]
async fn admin_token_file_is_private_and_removed_on_stop() {
    let mut f = Fixture::start("token-file");
    let mut c = f.client().await;
    let token_file = f.root.join("run").join("admin.token");
    let dacl = umr_platform::secure_file::file_dacl_sddl(&token_file).unwrap();
    let sid = umr_platform::identity::current_user_sid().unwrap();
    assert!(
        dacl.starts_with("D:P") && umr_platform::identity::sddl_names_user(&dacl, &sid),
        "{dacl}"
    );
    assert_eq!(dacl.matches("(A;").count(), 1, "{dacl}");

    let _: ShutdownAccepted = c.call(methods::SHUTDOWN, Value::Null).await.unwrap();
    drop(c);
    assert_eq!(f.wait_exit(), Some(0));
    assert!(
        !token_file.exists(),
        "admin token must be deleted on clean stop"
    );
}

#[tokio::test]
async fn scoped_client_token_works_over_the_pipe_and_survives_restart() {
    let mut f = Fixture::start("scoped");
    let mut admin = f.client().await;
    let created: CreateClientResult = admin
        .call(
            methods::AUTH_CLIENT_CREATE,
            json!({"name": "agent", "scopes": ["observe"]}),
        )
        .await
        .unwrap();

    let mut agent = Client::connect_with_token(&f.pipe, &created.token)
        .await
        .unwrap();
    assert!(agent.call_raw(methods::INFO, Value::Null).await.is_ok());
    match agent.call_raw(methods::SHUTDOWN, Value::Null).await {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::FORBIDDEN),
        other => panic!("expected forbidden, got {other:?}"),
    }
    let raw = std::fs::read_to_string(f.root.join("clients.json")).unwrap();
    assert!(
        !raw.contains(&created.token),
        "token must not be stored in clear text"
    );

    // restart the Core: the client token still works, the old admin token does not
    let old_admin = f.admin_token();
    let _: ShutdownAccepted = admin.call(methods::SHUTDOWN, Value::Null).await.unwrap();
    drop((admin, agent));
    assert_eq!(f.wait_exit(), Some(0));
    let f2 = Fixture {
        root: f.root.clone(),
        child: None,
        pipe: f.pipe.clone(),
    };
    std::mem::forget(f); // keep the folder; f2 cleans up
    let mut f2 = f2;
    f2.child = Some(
        Command::new(BIN)
            .arg("run")
            .env("UMR_HOME", &f2.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut c = f2.client_unauthenticated().await;
    // the new admin token may take a moment to be written; poll until it changes
    let start = Instant::now();
    while f2.admin_token() == old_admin && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        c.login(&old_admin).await.is_err(),
        "admin token is per Core start"
    );
    let mut agent = Client::connect_with_token(&f2.pipe, &created.token)
        .await
        .unwrap();
    assert!(agent.call_raw(methods::INFO, Value::Null).await.is_ok());
}
