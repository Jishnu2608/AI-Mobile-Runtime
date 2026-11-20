//! Live preview (Phase 4).
//!
//! | Module | Task |
//! |---|---|
//! | [`server`] | PRV-008 / PRV-010: localhost WebSocket for the preview window — emulator gRPC `streamScreenshot` frames out (RGBA at window size), input in (`streamInputEvent`) |
//! | [`stats`] | NFR-02a: time the product adds (input received → sent to the emulator, frame received → sent to the window, plus the window's own draw time) |
//! | [`selftest`] | PRV-011 / NFR-02c: touch → visible latency and frame rate, measured on the device |
//! | [`protocol`] | Messages between the Core and the preview window |
//!
//! Security (threat model T2/T3): the server listens on 127.0.0.1 only, every connection needs
//! a single-use token the Core hands out through its authenticated API, and browser origins
//! other than the desktop app's are refused (no DNS-rebinding or cross-site use).

pub mod protocol;
pub mod selftest;
pub mod server;
pub mod stats;

pub use server::{PreviewServer, Target};
