//! Device layer (Phase 2, steps 2.1-2.5).
//!
//! | Module | Task |
//! |---|---|
//! | [`adb`] | adb host protocol over TCP: `host:version`, `host:devices-l`, `host:track-devices-l`, `host:kill` |
//! | [`server`] | DEV-007: the product's **own adb server on a private port** (never 5037), started with the isolated environment; port choice; stop only our server |
//! | [`tracker`] | DEV-001: background device tracking with automatic reconnect |
//! | [`catalog`] | DEV-002/008: six size-class profiles (incl. foldables with hinge and postures) |
//! | [`avd`] | DEV-002: AVDs from our own hardware definitions, created lazily |
//! | [`emulator`] | DEV-003/009: headless boot with gRPC, boot wait, render health check, warm snapshot with GPU-mode record |
//! | [`health`] | DEV-009: black-screen detection on a raw `screencap` |
//! | [`console`] | Emulator text console (`avd snapshot save`, `kill`) |
//! | [`app`] | DEV-005: install / launch / stop / uninstall apps over adb |
//! | [`grpc`] | Emulator gRPC client: screenshot, touch, text, posture (DEC-007) |
//! | [`ui`] | DEV-006: compact UI tree (`uiautomator dump`) and adb input fallback |
//! | [`foreign`] | DEV-007: other adb servers on this machine (Android Studio, the user's own `adb`), reported and left alone |
//!
//! Why a private port: an adb client whose version differs from the running server kills and
//! restarts that server. Sharing port 5037 with Android Studio would therefore break the
//! user's debugging sessions whenever the two adb builds differ. On its own port the product's
//! server never touches theirs; emulators register with every adb server anyway, so devices are
//! visible on both sides.

pub mod adb;
pub mod app;
pub mod avd;
pub mod catalog;
pub mod console;
pub mod emulator;
pub mod foreign;
pub mod grpc;
pub mod health;
pub mod server;
pub mod tracker;
pub mod ui;

/// The well-known adb port (Android Studio, command-line `adb`). The product never sends
/// `host:kill` there.
pub const PUBLIC_ADB_PORT: u16 = 5037;
