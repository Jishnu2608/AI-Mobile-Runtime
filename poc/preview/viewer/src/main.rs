// POC-003 viewer (throwaway): Tauri 2 window that loads the bridge page.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    let auto = std::env::args().any(|a| a == "--auto");
    let url = if auto { "http://127.0.0.1:27200/?auto=1" } else { "http://127.0.0.1:27200/" };
    tauri::Builder::default()
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "preview", WebviewUrl::External(url.parse().unwrap()))
                .title("UMR POC-003 preview")
                .inner_size(420.0, 960.0)
                .always_on_top(false)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run failed");
}
