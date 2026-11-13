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
//! reboot.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::Value;
use tokio::sync::Mutex;
use umr_api::client::{Client, ClientError};
use umr_api::methods;
use umr_core::launcher::CoreTarget;

/// API methods the web view may call.
const ALLOWED: &[&str] = &[
    // preview window
    methods::PREVIEW_OPEN,
    methods::PREVIEW_STATUS,
    methods::PREVIEW_SELFTEST,
    methods::DEVICE_ROTATE,
    methods::DEVICE_POSTURE,
    methods::DEVICE_SCREENSHOT,
    methods::DEVICE_PROFILES,
    methods::SESSION_STATUS,
    methods::SESSION_START,
    methods::PROJECT_RELOAD,
    methods::PROJECT_STATUS,
    methods::DIAGNOSTICS_LIST,
    methods::DIAGNOSTICS_GET,
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
    let mut w = (h - bar) * aspect + 2.0 * margin;
    let max_w = ww / 3.0;
    if w > max_w {
        w = max_w;
        h = (w - 2.0 * margin) / aspect + bar;
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
    .title("AI Mobile Runtime — Preview")
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
        .title("AI Mobile Runtime — Setup")
        .inner_size(960.0, 720.0)
        .min_inner_size(720.0, 560.0)
        .build()?;
    Ok(())
}

fn main() {
    let bridge = Bridge {
        target: CoreTarget::resolve(),
        client: Mutex::new(None),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preview = mode(&args);
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
            wizard_save
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| eprintln!("umr-desktop: {e}"));
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
