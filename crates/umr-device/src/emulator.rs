//! DEV-003 / DEV-009: headless emulator boot.
//!
//! * Start `emulator -avd <name> -no-window -gpu host -port <console> -grpc <port>
//!   -grpc-use-token` with the isolated environment through the Supervisor (DEV-004): ports
//!   from the port allocator, process tree in a kill-on-close Job Object (no orphaned `qemu`
//!   processes if the Core dies), restart policy `Never` (a crash is a device error).
//! * Wait until the device is online on the product's adb server and `sys.boot_completed=1`.
//! * Render health check ([`crate::health`]): a black launcher means a broken GPU session.
//! * Warm boot: after a healthy cold boot the product saves the snapshot `umr_warm` and records
//!   the GPU mode and emulator version next to it. The next boot loads the snapshot only when
//!   both still match; otherwise (or when the health check fails after a warm boot) it boots
//!   cold and saves a fresh snapshot. POC-006 finding 1.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use umr_supervisor::{Lease, PortAllocator, PortRange, ProcessSpec, RestartPolicy, Supervisor};

use crate::adb;
use crate::console::Console;
use crate::health::{self, Health};

/// Name of the product's warm-boot snapshot inside each AVD.
pub const SNAPSHOT: &str = "umr_warm";
/// Sidecar file in the AVD folder: how the snapshot was taken.
pub const RECORD_FILE: &str = "umr-snapshot.json";
/// GPU modes the product boots with: hardware (DEC-006) and the software fallbacks.
pub const GPU_MODES: [&str; 3] = ["host", "swiftshader_indirect", "angle_indirect"];

/// How the warm-boot snapshot was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub name: String,
    pub gpu: String,
    pub emulator: String,
    pub saved_unix: u64,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[must_use]
pub fn read_record(avd_dir: &Path) -> Option<SnapshotRecord> {
    let text = std::fs::read_to_string(avd_dir.join(RECORD_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_record(avd_dir: &Path, r: &SnapshotRecord) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(r).map_err(std::io::Error::other)?;
    std::fs::write(avd_dir.join(RECORD_FILE), text)
}

pub fn forget_record(avd_dir: &Path) {
    let _ = std::fs::remove_file(avd_dir.join(RECORD_FILE));
}

/// `Ok` when the warm snapshot may be loaded; otherwise why the boot must be cold.
pub fn warm_usable(avd_dir: &Path, gpu: &str, emulator: &str) -> Result<SnapshotRecord, String> {
    let r = read_record(avd_dir).ok_or("no warm snapshot yet")?;
    if r.gpu != gpu {
        return Err(format!(
            "the snapshot was taken with GPU mode '{}', this boot uses '{gpu}'",
            r.gpu
        ));
    }
    if r.emulator != emulator {
        return Err(format!(
            "the snapshot was taken with emulator {}, now {emulator}",
            r.emulator
        ));
    }
    if !avd_dir.join("snapshots").join(&r.name).is_dir() {
        return Err("the snapshot files are missing".into());
    }
    Ok(r)
}

/// `Pkg.Revision` of the emulator package (`<sdk>/emulator/source.properties`).
#[must_use]
pub fn emulator_version(emulator_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(emulator_dir.join("source.properties")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("Pkg.Revision="))
        .map(|v| v.trim().to_string())
}

/// Console ports of emulators adb already knows (`emulator-5554` → 5554), so a user's own
/// emulator that just closed its port is not reused immediately.
#[must_use]
pub fn console_ports_in_use(serials: &[String]) -> Vec<u16> {
    serials
        .iter()
        .filter_map(|s| s.strip_prefix("emulator-")?.parse().ok())
        .collect()
}

/// `key=value` lines of an emulator discovery file (`pid_<n>.ini`).
#[must_use]
pub fn parse_discovery(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// The discovery file of the emulator on `console_port` (gRPC port and token).
#[must_use]
pub fn find_discovery(dirs: &[PathBuf], console_port: u16) -> Option<HashMap<String, String>> {
    let want = console_port.to_string();
    dirs.iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("pid_"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .map(|t| parse_discovery(&t))
        .find(|m| m.get("port.serial") == Some(&want))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BootMode {
    Cold,
    Warm,
}

/// What a boot did and how long it took.
#[derive(Debug, Clone, Serialize)]
pub struct BootReport {
    pub avd: String,
    pub serial: String,
    pub console_port: u16,
    pub grpc_port: Option<u16>,
    /// Token for the emulator's gRPC endpoint; never shown to clients.
    #[serde(skip)]
    pub grpc_token: Option<String>,
    pub gpu: String,
    pub mode: BootMode,
    /// Why the boot was cold (no snapshot, GPU / version mismatch, failed health check, ...).
    pub cold_reason: Option<String>,
    /// From process start until the health check passed.
    pub seconds: f64,
    /// From process start until `sys.boot_completed=1`.
    pub boot_completed_seconds: f64,
    pub health: Health,
    pub snapshot_saved: bool,
    /// Launches needed (2 when a warm boot had to be redone cold).
    pub attempts: u32,
    pub log: String,
}

#[derive(Debug, Clone)]
pub struct BootOptions {
    pub supervisor: Supervisor,
    pub ports: PortAllocator,
    pub emulator_exe: PathBuf,
    pub avd: String,
    pub avd_dir: PathBuf,
    /// Isolated child environment (incl. `ANDROID_ADB_SERVER_PORT` of the product's server).
    pub env: HashMap<String, String>,
    pub adb_port: u16,
    pub log_dir: PathBuf,
    pub discovery_dirs: Vec<PathBuf>,
    pub gpu: String,
    pub force_cold: bool,
    /// Show the emulator's own window (debugging); the product preview is headless.
    pub window: bool,
    pub save_snapshot: bool,
    pub cold_timeout: Duration,
    pub warm_timeout: Duration,
}

impl BootOptions {
    /// Defaults: `-gpu host` (DEC-006), headless, snapshot saving on, 5 min cold / 90 s warm.
    #[must_use]
    pub fn new(emulator_exe: &Path, avd: &str, avd_dir: &Path, adb_port: u16) -> Self {
        Self {
            supervisor: Supervisor::new(),
            ports: PortAllocator::new(),
            emulator_exe: emulator_exe.to_path_buf(),
            avd: avd.to_string(),
            avd_dir: avd_dir.to_path_buf(),
            env: HashMap::new(),
            adb_port,
            log_dir: std::env::temp_dir(),
            discovery_dirs: Vec::new(),
            gpu: "host".into(),
            force_cold: false,
            window: false,
            save_snapshot: true,
            cold_timeout: Duration::from_secs(300),
            warm_timeout: Duration::from_secs(90),
        }
    }

    /// Command-line arguments for one launch.
    #[must_use]
    pub fn args(&self, console: u16, grpc: u16, warm: bool) -> Vec<String> {
        let mut a: Vec<String> = [
            "-avd",
            &self.avd,
            "-gpu",
            &self.gpu,
            "-no-audio",
            "-no-boot-anim",
            "-port",
            &console.to_string(),
            "-grpc",
            &grpc.to_string(),
            "-grpc-use-token",
            // the product saves its own snapshot explicitly, so quick-boot never overwrites it
            // with an unchecked state on exit
            "-no-snapshot-save",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        if !self.window {
            a.push("-no-window".into());
        }
        if warm {
            a.extend(["-snapshot".into(), SNAPSHOT.into()]);
        } else {
            a.push("-no-snapshot-load".into());
        }
        a
    }
}

/// Supervisor name of an AVD's emulator process.
#[must_use]
pub fn process_name(avd: &str) -> String {
    format!("emulator:{avd}")
}

/// A running emulator started by the product. Holds its port leases while it runs.
#[derive(Debug)]
pub struct Instance {
    supervisor: Supervisor,
    name: String,
    _leases: Vec<Lease>,
    pub report: BootReport,
}

impl Instance {
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.supervisor.status(&self.name).and_then(|s| s.pid)
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.supervisor.is_running(&self.name)
    }

    /// Asks the emulator to quit (console `kill`), then ends the process tree if it does not.
    pub fn stop(self) {
        if self.is_running() {
            if let Ok(mut c) = Console::connect(self.report.console_port, Duration::from_secs(3)) {
                let _ = c.command("kill");
            }
            let _ = self
                .supervisor
                .wait_exit(&self.name, Duration::from_secs(15));
        }
        // ends qemu and any other member of the job, then forgets the process
        self.supervisor.remove(&self.name);
    }
}

fn log_tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn known_serials(adb_port: u16) -> Vec<String> {
    adb::devices(adb_port)
        .map(|d| d.into_iter().map(|x| x.serial).collect())
        .unwrap_or_default()
}

/// One frame for the health check: gRPC RGBA (~0.1 s) when available, else a raw adb
/// `screencap` (~1 s for 10 MB).
fn grab(adb_port: u16, serial: &str, grpc: Option<&crate::grpc::Emulator>) -> Health {
    if let Some(g) = grpc {
        if let Ok((px, w, h)) = g.screenshot_rgba() {
            return health::assess_pixels(w, h, &px);
        }
    }
    match adb::shell_bytes(adb_port, serial, "screencap", Duration::from_secs(20)) {
        Ok(raw) => health::assess(&raw),
        Err(e) => Health {
            problem: Some(format!("screencap failed: {e}")),
            ..Health::default()
        },
    }
}

/// Waits (up to 12 s) for a healthy launcher screen; returns the last assessment.
fn check_render(adb_port: u16, serial: &str, grpc: Option<&crate::grpc::Emulator>) -> Health {
    let _ = adb::shell(adb_port, serial, "input keyevent KEYCODE_WAKEUP");
    let _ = adb::shell(adb_port, serial, "input keyevent KEYCODE_HOME");
    let start = Instant::now();
    let mut last = Health::default();
    while start.elapsed() < Duration::from_secs(12) {
        std::thread::sleep(Duration::from_millis(400));
        last = grab(adb_port, serial, grpc);
        if last.ok {
            break;
        }
    }
    last
}

fn secs(d: Duration) -> f64 {
    (d.as_secs_f64() * 10.0).round() / 10.0
}

fn launch(opts: &BootOptions, warm: bool) -> Result<Instance, String> {
    let owner = process_name(&opts.avd);
    let in_use = console_ports_in_use(&known_serials(opts.adb_port));
    let console_lease = opts.ports.lease(&owner, PortRange::EMULATOR, &in_use)?;
    let grpc_lease = opts.ports.lease(&owner, PortRange::GRPC, &[])?;
    let (console, grpc) = (console_lease.port, grpc_lease.port);
    let log = opts.log_dir.join(format!("emulator-{}.log", opts.avd));
    let mut spec =
        ProcessSpec::new(&owner, &opts.emulator_exe, &log).args(opts.args(console, grpc, warm));
    spec.env.clone_from(&opts.env);
    spec.restart = RestartPolicy::Never;
    let start = Instant::now();
    opts.supervisor
        .start(spec)
        .map_err(|e| format!("cannot start the emulator: {e}"))?;
    let serial = format!("emulator-{console}");
    let mut inst = Instance {
        supervisor: opts.supervisor.clone(),
        name: owner.clone(),
        _leases: vec![console_lease, grpc_lease],
        report: BootReport {
            avd: opts.avd.clone(),
            serial: serial.clone(),
            console_port: console,
            grpc_port: Some(grpc),
            grpc_token: None,
            gpu: opts.gpu.clone(),
            mode: if warm { BootMode::Warm } else { BootMode::Cold },
            cold_reason: None,
            seconds: 0.0,
            boot_completed_seconds: 0.0,
            health: Health::default(),
            snapshot_saved: false,
            attempts: 1,
            log: log.display().to_string(),
        },
    };
    let timeout = if warm {
        opts.warm_timeout
    } else {
        opts.cold_timeout
    };
    loop {
        if !inst.is_running() {
            let code = opts
                .supervisor
                .status(&owner)
                .and_then(|s| s.exit_code)
                .map_or_else(|| "no exit code".to_string(), |c| format!("exit code {c}"));
            opts.supervisor.remove(&owner);
            return Err(format!(
                "the emulator stopped during boot ({code}); last log lines:\n{}",
                log_tail(&log, 15)
            ));
        }
        if start.elapsed() > timeout {
            inst.stop();
            return Err(format!(
                "the emulator did not finish booting within {} s; log: {}",
                timeout.as_secs(),
                log.display()
            ));
        }
        let online = adb::devices(opts.adb_port)
            .is_ok_and(|d| d.iter().any(|x| x.serial == serial && x.is_ready()));
        if online
            && adb::shell(opts.adb_port, &serial, "getprop sys.boot_completed")
                .is_ok_and(|v| v == "1")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(if online { 500 } else { 1000 }));
    }
    inst.report.boot_completed_seconds = secs(start.elapsed());
    if let Some(d) = find_discovery(&opts.discovery_dirs, console) {
        inst.report.grpc_port = d
            .get("grpc.port")
            .and_then(|p| p.parse().ok())
            .or(Some(grpc));
        inst.report.grpc_token = d.get("grpc.token").cloned();
    }
    let grpc = inst
        .report
        .grpc_port
        .map(|p| crate::grpc::Emulator::new(p, inst.report.grpc_token.clone()));
    inst.report.health = check_render(opts.adb_port, &serial, grpc.as_ref());
    inst.report.seconds = secs(start.elapsed());
    Ok(inst)
}

/// The emulator logs this when `-snapshot <name>` could not be restored; it then cold-boots.
#[must_use]
pub fn snapshot_load_failed(log: &str) -> bool {
    log.contains(&format!("Failed to load snapshot '{SNAPSHOT}'"))
}

/// Saves the warm snapshot of a healthy cold boot and records how it was taken.
fn save_snapshot(opts: &BootOptions, inst: &mut Instance, emulator: &str) -> Result<(), String> {
    let mut c = Console::connect(inst.report.console_port, Duration::from_secs(5))?;
    c.set_timeout(Duration::from_secs(180));
    c.command(&format!("avd snapshot save {SNAPSHOT}"))?;
    write_record(
        &opts.avd_dir,
        &SnapshotRecord {
            name: SNAPSHOT.into(),
            gpu: opts.gpu.clone(),
            emulator: emulator.to_string(),
            saved_unix: now_unix(),
        },
    )
    .map_err(|e| e.to_string())?;
    inst.report.snapshot_saved = true;
    Ok(())
}

/// Boots the AVD: warm when the recorded snapshot matches, cold otherwise; always health-checked.
pub fn boot(opts: &BootOptions) -> Result<Instance, String> {
    let emu_dir = opts
        .emulator_exe
        .parent()
        .ok_or("emulator path has no folder")?;
    let version = emulator_version(emu_dir).unwrap_or_else(|| "unknown".into());
    let warm = if opts.force_cold {
        Err("cold boot requested".to_string())
    } else {
        warm_usable(&opts.avd_dir, &opts.gpu, &version)
    };
    let mut cold_reason = warm.as_ref().err().cloned();
    let mut attempts = 1;
    if warm.is_ok() {
        match launch(opts, true) {
            Ok(mut inst) if inst.report.health.ok => {
                inst.report.attempts = attempts;
                let log = std::fs::read_to_string(&inst.report.log).unwrap_or_default();
                if !snapshot_load_failed(&log) {
                    return Ok(inst);
                }
                // The emulator rejected the snapshot itself (it stores the GPU configuration)
                // and booted cold: report it as such and replace the snapshot.
                inst.report.mode = BootMode::Cold;
                inst.report.cold_reason =
                    Some("the emulator could not load the snapshot and booted cold".into());
                forget_record(&opts.avd_dir);
                if opts.save_snapshot {
                    if let Err(e) = save_snapshot(opts, &mut inst, &version) {
                        inst.report.cold_reason = Some(format!(
                            "the emulator could not load the snapshot and booted cold; saving a new one failed: {e}"
                        ));
                    }
                }
                return Ok(inst);
            }
            Ok(inst) => {
                cold_reason = Some(format!(
                    "the screen was black after loading the snapshot ({}); booted cold instead",
                    inst.report.health.problem.clone().unwrap_or_default()
                ));
                inst.stop();
            }
            Err(e) => cold_reason = Some(format!("loading the snapshot failed: {e}")),
        }
        forget_record(&opts.avd_dir);
        attempts += 1;
    }
    let mut inst = launch(opts, false)?;
    inst.report.cold_reason = cold_reason;
    inst.report.attempts = attempts;
    if !inst.report.health.ok {
        let problem = inst.report.health.problem.clone().unwrap_or_default();
        inst.stop();
        return Err(format!(
            "render health check failed after a cold boot: {problem}"
        ));
    }
    if opts.save_snapshot {
        if let Err(e) = save_snapshot(opts, &mut inst, &version) {
            inst.report.cold_reason = Some(format!(
                "{}; saving the warm snapshot failed: {e}",
                inst.report.cold_reason.clone().unwrap_or_default()
            ));
        }
    }
    Ok(inst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-emu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn warm_snapshot_only_when_gpu_and_version_match() {
        let d = tmp("warm");
        assert!(warm_usable(&d, "host", "37.2.13")
            .unwrap_err()
            .contains("no warm snapshot"));
        let r = SnapshotRecord {
            name: SNAPSHOT.into(),
            gpu: "host".into(),
            emulator: "37.2.13".into(),
            saved_unix: 1,
        };
        write_record(&d, &r).unwrap();
        assert!(warm_usable(&d, "host", "37.2.13")
            .unwrap_err()
            .contains("missing"));
        std::fs::create_dir_all(d.join("snapshots").join(SNAPSHOT)).unwrap();
        assert_eq!(warm_usable(&d, "host", "37.2.13").unwrap(), r);
        assert!(warm_usable(&d, "swiftshader_indirect", "37.2.13")
            .unwrap_err()
            .contains("GPU mode 'host'"));
        assert!(warm_usable(&d, "host", "38.0.1")
            .unwrap_err()
            .contains("emulator 37.2.13"));
        forget_record(&d);
        assert!(read_record(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn detects_a_rejected_snapshot_in_the_log() {
        let failed = "INFO | Loading snapshot 'umr_warm'...\nWARNING | Error -1 from the snapshot callback\nWARNING | Failed to load snapshot 'umr_warm'\n";
        assert!(snapshot_load_failed(failed));
        assert!(!snapshot_load_failed(
            "INFO | Loading snapshot 'umr_warm'...\nINFO | Boot completed\n"
        ));
    }

    #[test]
    fn arguments_for_cold_and_warm_boots() {
        let o = BootOptions::new(
            Path::new("emulator.exe"),
            "umr_standard_api36",
            Path::new("x"),
            5137,
        );
        let cold = o.args(5556, 8554, false).join(" ");
        assert!(cold.contains("-avd umr_standard_api36 -gpu host"));
        assert!(cold.contains("-port 5556 -grpc 8554 -grpc-use-token"));
        assert!(cold.contains("-no-window") && cold.ends_with("-no-snapshot-load"));
        assert!(cold.contains("-no-snapshot-save"));
        let warm = o.args(5554, 8554, true).join(" ");
        assert!(warm.ends_with("-snapshot umr_warm"));
    }

    #[test]
    fn version_and_discovery_files() {
        let d = tmp("disc");
        std::fs::write(
            d.join("source.properties"),
            "Pkg.UserSrc=false\nPkg.Revision=37.2.13\n",
        )
        .unwrap();
        assert_eq!(emulator_version(&d).as_deref(), Some("37.2.13"));
        std::fs::write(
            d.join("pid_100.ini"),
            "port.serial=5554\nport.adb=5555\ngrpc.port=8554\ngrpc.token=abc\n",
        )
        .unwrap();
        std::fs::write(
            d.join("pid_200.ini"),
            "port.serial=5556\ngrpc.port=8555\ngrpc.token=def\n",
        )
        .unwrap();
        let m = find_discovery(std::slice::from_ref(&d), 5556).unwrap();
        assert_eq!(m["grpc.token"], "def");
        assert!(find_discovery(std::slice::from_ref(&d), 5600).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn console_ports_known_to_adb() {
        let s = [
            "emulator-5554".to_string(),
            "R58M123".into(),
            "emulator-5560".into(),
        ];
        assert_eq!(console_ports_in_use(&s), [5554, 5560]);
    }
}
