//! Desktop app (UI-001/002, Phase 4): a Tauri 2 window that talks to the Runtime Core.
//!
//! * no arguments: the onboarding wizard (Scan → Project → Plan → Licences → Install → Ready);
//! * `--preview [--dock right|left|float]`: the live preview window (PRV-005/006) — WebGL2 frames
//!   from the Core's preview server, input back, toolbar, docked at a screen edge beside the
//!   coding agent's terminal.
//!
//! The app is a thin client, like the CLI: it starts the Core if needed, logs in with the
//! Core's admin token and forwards calls. Only the API methods the wizard needs can be called
//! from the web view ([`ALLOWED`]), so a bug in the page cannot reach anything else.
//! The wizard's position is saved in `<home>\wizard.json`, so it continues after a restart or
//! reboot. Its last step connects coding agents (CON-005, `agents.rs`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod links;
mod open;
mod updates;

use serde_json::Value;
use tokio::sync::Mutex;
use tumr_api::client::{Client, ClientError};
use tumr_api::methods;
use tumr_core::launcher::CoreTarget;

/// API methods the web view may call.
const ALLOWED: &[&str] = &[
    // preview window
    methods::PREVIEW_OPEN,
    methods::PREVIEW_STATUS,
    methods::PREVIEW_SELFTEST,
    methods::DEVICE_ROTATE,
    methods::DEVICE_FINGERPRINT,
    methods::DEVICE_AUTH_STATE,
    methods::DEVICE_POSTURE,
    methods::DEVICE_SCREENSHOT,
    methods::DEVICE_PROFILES,
    methods::DEVICE_PREPARE,
    methods::SESSION_STATUS,
    methods::SESSION_START,
    methods::PROJECT_RELOAD,
    methods::PROJECT_STOP,
    methods::PROJECT_STATUS,
    // wizard step "Run" (Phase 9.6)
    methods::PROJECT_RUN,
    methods::DIAGNOSTICS_LIST,
    methods::DIAGNOSTICS_GET,
    methods::DIAGNOSTICS_FLAG,
    methods::HOME_SUMMARY,
    methods::LOGS_TAIL,
    methods::APPROVALS_LIST,
    methods::APPROVALS_DECIDE,
    // wizard
    methods::INFO,
    methods::DOCTOR,
    methods::ENV_SCAN,
    methods::PROJECT_DETECT,
    methods::PROJECT_TRUST,
    methods::PROJECT_PLAN,
    methods::INSTALL_PREPARE,
    methods::INSTALL_ACCEPT_LICENCE,
    methods::INSTALL_START,
    methods::INSTALL_STATUS,
    methods::INSTALL_CANCEL,
    methods::COMPONENTS_LIST,
    methods::ADMIN_RUN,
];

struct Bridge {
    target: Result<CoreTarget, String>,
    client: Mutex<Option<Client>>,
}

impl Bridge {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let target = self.target.as_ref().map_err(Clone::clone)?;
        // AUD-P1-6: a run or a device switch takes minutes; on the shared connection every other
        // call (progress, Cancel, the problems) would wait behind it. They get their own.
        if matches!(method, methods::PROJECT_RUN | methods::SESSION_START) {
            let (mut c, _) = target.connect_or_start().await?;
            return match c.call_raw(method, params).await {
                Ok(v) => Ok(v),
                Err(ClientError::Rpc(e)) => Err(e.message),
                Err(e) => Err(e.to_string()),
            };
        }
        let mut slot = self.client.lock().await;
        for attempt in 0..2 {
            if slot.is_none() {
                let (c, _) = target.connect_or_start().await?;
                *slot = Some(c);
            }
            let Some(client) = slot.as_mut() else {
                continue;
            };
            match client.call_raw(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(ClientError::Rpc(e)) => return Err(e.message),
                Err(e) if attempt == 0 => {
                    // connection lost (Core restarted): reconnect once
                    let _ = e;
                    *slot = None;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("cannot reach the Runtime Core".into())
    }

    fn wizard_file(&self) -> Result<std::path::PathBuf, String> {
        let t = self.target.as_ref().map_err(Clone::clone)?;
        Ok(t.paths.root().join("wizard.json"))
    }
}

#[tauri::command]
async fn core_call(
    bridge: tauri::State<'_, Bridge>,
    method: String,
    params: Value,
) -> Result<Value, String> {
    if !ALLOWED.contains(&method.as_str()) {
        return Err(format!(
            "method '{method}' is not available to the desktop window"
        ));
    }
    bridge.call(&method, params).await
}

#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
#[tauri::command]
fn wizard_load(bridge: tauri::State<'_, Bridge>) -> Result<Value, String> {
    let f = bridge.wizard_file()?;
    match std::fs::read_to_string(&f) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| e.to_string()),
        Err(_) => Ok(Value::Null),
    }
}

#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
#[tauri::command]
fn wizard_save(bridge: tauri::State<'_, Bridge>, state: Value) -> Result<(), String> {
    let f = bridge.wizard_file()?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &f,
        serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// The "Connect your agent" page (CON-005): runs `tumr agent … --json` (see `agents.rs`).
#[tauri::command]
async fn agent_cli(request: agents::AgentRequest) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || agents::run(&request))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens a user guide page in the browser (fixed pages and anchors, see `links.rs`).
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
#[tauri::command]
fn open_docs(page: String, anchor: Option<String>) -> Result<(), String> {
    links::open_docs(&page, anchor.as_deref())
}

/// Opens the docked live preview window.
/// UPD-001: is a newer version published on the configured channel?
#[tauri::command]
async fn update_check() -> Result<Value, String> {
    tokio::task::spawn_blocking(updates::check)
        .await
        .map_err(|e| e.to_string())?
}

/// UPD-001: download, verify and install the newer version; the app closes and opens again.
#[tauri::command]
fn update_apply() -> Result<(), String> {
    updates::apply()
}

/// Windows' own folder picker for the project step; `None` when cancelled. The page receives the
/// chosen path only (no file system access for the web view).
#[tauri::command]
async fn pick_folder(
    window: tauri::WebviewWindow,
    start: Option<String>,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Choose your app's project folder")
        .set_parent(&window);
    if let Some(dir) = start.map(std::path::PathBuf::from).filter(|p| p.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    Ok(dialog
        .pick_folder()
        .await
        .map(|f| f.path().display().to_string()))
}

#[tauri::command]
fn open_preview_window() -> Result<(), String> {
    links::open_preview()
}

/// Where the preview window sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dock {
    Right,
    Left,
    Float,
}

impl Dock {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            "float" => Some(Self::Float),
            _ => None,
        }
    }
}

/// Command line: `--preview [--dock right|left|float]`.
fn mode(args: &[String]) -> Option<Dock> {
    if !args.iter().any(|a| a == "--preview") {
        return None;
    }
    let dock = args
        .iter()
        .position(|a| a == "--dock")
        .and_then(|i| args.get(i + 1))
        .and_then(|d| Dock::parse(d))
        .unwrap_or(Dock::Right);
    Some(dock)
}

/// Preview window rectangle (physical pixels) inside a monitor's work area `(x, y, w, h)`:
/// full height minus a margin, phone aspect plus the toolbar, at most a third of the width
/// (AC-CLI-02: the rest of the screen stays free for the terminal / editor).
fn dock_rect(work: (i32, i32, u32, u32), dock: Dock, scale: f64) -> (i32, i32, u32, u32) {
    let (wx, wy, ww, wh) = (
        f64::from(work.0),
        f64::from(work.1),
        f64::from(work.2),
        f64::from(work.3),
    );
    let margin = 8.0 * scale;
    let bar = 48.0 * scale; // toolbar
    let aspect = 1080.0 / 2400.0;
    let mut h = wh - 2.0 * margin;
    // AUD-P2-21: wide enough for the toolbar on one row (it wrapped at 358 px)
    let mut w = ((h - bar) * aspect + 2.0 * margin).max(500.0 * scale);
    // a third of the screen, or a little more when a third is too narrow for the toolbar
    let max_w = (ww / 3.0).max((500.0 * scale).min(ww * 0.36));
    if w > max_w {
        w = max_w;
        h = h.min((w - 2.0 * margin) / aspect + bar);
    }
    let x = match dock {
        Dock::Right => wx + ww - w - margin,
        Dock::Left => wx + margin,
        Dock::Float => wx + (ww - w) / 2.0,
    };
    let y = wy + (wh - h) / 2.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (
        x.round() as i32,
        y.round() as i32,
        w.round() as u32,
        h.round() as u32,
    )
}

fn open_preview(app: &tauri::App, dock: Dock) -> tauri::Result<()> {
    let win = tauri::WebviewWindowBuilder::new(
        app,
        "preview",
        tauri::WebviewUrl::App("index.html?view=preview".into()),
    )
    .title("Phone — Toolchain Universal Mobile Runtime")
    .inner_size(420.0, 860.0)
    .min_inner_size(240.0, 400.0)
    .visible(false)
    .build()?;
    if let Some(m) = win.primary_monitor()? {
        let wa = m.work_area();
        let (x, y, w, h) = dock_rect(
            (wa.position.x, wa.position.y, wa.size.width, wa.size.height),
            dock,
            m.scale_factor(),
        );
        // dock_rect is the outer frame; the title bar and borders come off the inner size
        let (outer, inner) = (win.outer_size()?, win.inner_size()?);
        let dw = outer.width.saturating_sub(inner.width);
        let dh = outer.height.saturating_sub(inner.height);
        win.set_size(tauri::PhysicalSize::new(
            w.saturating_sub(dw),
            h.saturating_sub(dh),
        ))?;
        win.set_position(tauri::PhysicalPosition::new(x, y))?;
    }
    win.show()?;
    Ok(())
}

fn open_wizard(app: &tauri::App) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .title("Toolchain Universal Mobile Runtime")
        .inner_size(960.0, 720.0)
        .min_inner_size(720.0, 560.0)
        .build()?;
    Ok(())
}

/// AUD-P1-5: one preview window. A second `--preview` (another Run, "Open the live preview",
/// `tumr preview`) brings the running one to the front and ends instead of opening a copy.
fn preview_already_open(root: &std::path::Path) -> bool {
    let file = root.join("run").join("preview.pid");
    let me = std::process::id();
    let other = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .filter(|pid| *pid != me);
    if let Some(pid) = other {
        let is_preview = tumr_platform::process::list_processes().is_ok_and(|all| {
            all.iter()
                .any(|p| p.pid == pid && p.exe_name.eq_ignore_ascii_case("tumr-desktop.exe"))
        });
        if is_preview && tumr_platform::window::focus_window_of(pid) {
            return true;
        }
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, me.to_string());
    false
}

/// AUD-P1-5: the window's title names what it shows ("Phone — rncalc"), so the setup window and
/// the phone are told apart in the taskbar and Alt+Tab.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
fn set_window_title(window: tauri::WebviewWindow, title: String) {
    let t: String = title.chars().take(80).collect();
    let _ = window.set_title(&t);
}

/// AUD-P2-20 / AUD-P3-23: opens a problem's file at its line, or an install log (text files
/// only; VS Code, else Notepad). Returns the program's name.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
fn open_in_editor(path: String, line: Option<u32>, root: Option<String>) -> Result<String, String> {
    let path = match root {
        Some(r) => open::resolve(std::path::Path::new(&r), &path)?,
        None => path,
    };
    open::open_in_editor(&path, line).map(str::to_string)
}

/// AUD-P2-21: the problems panel opens beside the phone; the window grows sideways by `extra`
/// logical pixels (toward the free side of the screen) so the phone keeps its size. A negative
/// `extra` gives the room back. Returns the logical pixels actually added.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
fn widen_window(window: tauri::WebviewWindow, extra: f64) -> Result<f64, String> {
    let e = |x: tauri::Error| x.to_string();
    let scale = window.scale_factor().map_err(e)?;
    let outer = window.outer_size().map_err(e)?;
    let inner = window.inner_size().map_err(e)?;
    let pos = window.outer_position().map_err(e)?;
    let (wx, ww) = window
        .current_monitor()
        .map_err(e)?
        .map_or((pos.x, outer.width + 4000), |m| {
            let wa = m.work_area();
            (wa.position.x, wa.size.width)
        });
    let right = wx + i32::try_from(ww).unwrap_or(i32::MAX);
    #[allow(clippy::cast_possible_truncation)]
    let want = (extra * scale).round() as i32;
    // set_size takes the inner size; the frame (borders) stays the same
    let frame = i32::try_from(outer.width.saturating_sub(inner.width)).unwrap_or(0);
    let width = i32::try_from(inner.width).unwrap_or(i32::MAX);
    let new_w = (width + want).clamp(240, (right - wx - frame).max(240));
    let added = new_w - width;
    // docked at the right edge: grow to the left; otherwise to the right while there is room
    let at_right = pos.x + width + frame >= right - 16;
    let mut x = if at_right { pos.x - added } else { pos.x };
    x = x.clamp(wx, (right - new_w - frame).max(wx));
    #[allow(clippy::cast_sign_loss)]
    window
        .set_size(tauri::PhysicalSize::new(new_w as u32, inner.height))
        .map_err(e)?;
    window
        .set_position(tauri::PhysicalPosition::new(x, pos.y))
        .map_err(e)?;
    Ok(f64::from(added) / scale)
}

fn main() {
    let bridge = Bridge {
        target: CoreTarget::resolve(),
        client: Mutex::new(None),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preview = mode(&args);
    if preview.is_some() {
        if let Ok(t) = &bridge.target {
            if preview_already_open(t.paths.root()) {
                return;
            }
        }
    }
    tauri::Builder::default()
        .manage(bridge)
        .setup(move |app| {
            match preview {
                Some(dock) => open_preview(app, dock)?,
                None => open_wizard(app)?,
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            core_call,
            wizard_load,
            wizard_save,
            agent_cli,
            open_docs,
            open_preview_window,
            pick_folder,
            update_check,
            update_apply,
            set_window_title,
            open_in_editor,
            widen_window
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| eprintln!("tumr-desktop: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn preview_arguments_and_docking() {
        let a = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(mode(&a(&[])), None);
        assert_eq!(mode(&a(&["--preview"])), Some(Dock::Right));
        assert_eq!(mode(&a(&["--preview", "--dock", "left"])), Some(Dock::Left));
        assert_eq!(
            mode(&a(&["--preview", "--dock", "bogus"])),
            Some(Dock::Right)
        );
        // 1920x1040 work area at 100 %: right edge, full height, a third of the width at most
        let (x, y, w, h) = dock_rect((0, 0, 1920, 1040), Dock::Right, 1.0);
        assert!(w <= 640 && h <= 1040, "{w}x{h}");
        assert_eq!(
            x + i32::try_from(w).unwrap() + 8,
            1920,
            "flush with the right edge"
        );
        assert!(y >= 0);
        let (lx, ..) = dock_rect((0, 0, 1920, 1040), Dock::Left, 1.0);
        assert_eq!(lx, 8);
        // 4K at 150 %
        let (_, _, w4, h4) = dock_rect((0, 0, 3840, 2100), Dock::Right, 1.5);
        assert!(w4 <= 1280 && h4 <= 2100);
    }

    #[test]
    fn only_wizard_methods_are_allowed() {
        assert!(ALLOWED.contains(&methods::INSTALL_START));
        for m in [
            methods::SHUTDOWN,
            methods::AUTH_CLIENT_CREATE,
            methods::AUTH_CLIENT_LIST,
            methods::COMPONENTS_REMOVE,
            methods::COMPONENTS_REPAIR,
        ] {
            assert!(
                !ALLOWED.contains(&m),
                "{m} must not be reachable from the web view"
            );
        }
    }
}
