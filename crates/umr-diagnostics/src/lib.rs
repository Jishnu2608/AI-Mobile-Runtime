//! Diagnostics collectors (Phase 5 step 5.1: DIA-001, DIA-006).
//!
//! | Source | Collector |
//! |---|---|
//! | Gradle build output | already collected: `adapters/gradle-android` reads the tool's own log and `adapters/sdk::problems::parse_line` extracts kotlinc/javac/AAPT errors with file and line |
//! | Flutter machine mode | already collected: `adapters/flutter` decodes `flutter run --machine`'s JSON events (`app.log`, `daemon.logMessage`) and the same `problems::parse_line` reads Dart compiler errors from them |
//! | Metro / React Native | already collected: `adapters/react-native` follows Metro's stdout; `problems::parse_line` reads Babel/Metro syntax errors and marks unlocated JS runtime errors |
//! | Android crash buffer (`logcat -b crash`) | **[`crash`]**, new here: Java/Kotlin `FATAL EXCEPTION` blocks and native (tombstone) crash headers — nothing else reads this buffer |
//! | ANR traces | **[`crash::parse_anr`]**, new here: `ActivityManager` "ANR in …" blocks from the main log |
//!
//! Scope (DIA-006): each parser is proven against seeded fault text (unit tests) and, for the
//! crash buffer, against a real crash captured live on the emulator (`adb shell am crash`).
//! Step 5.2: [`classify`] turns any collector's output (adapter [`umr_adapter::Problem`]s and
//! errors, [`CrashEvent`], [`AnrEvent`], free text) into a [`Diagnostic`] with one of six
//! [`Category`]s and an [`Origin`] (app code or environment) — DIA-002; [`fingerprint`] gives
//! repeats one identity and [`Dedupe`] keeps one counted entry per problem — DIA-003.
//!
//! Step 5.4: [`sanitize`] masks secrets and personal data in any text that leaves the Core
//! (DIA-005). The Core keeps the per-run store and serves it (`diagnostics.*`, `logs.tail`,
//! DIA-004); the ≥ 60-case corpus is `tests/corpus.rs` (TEST-006).

pub mod classify;
pub mod crash;
pub mod fingerprint;
pub mod model;
pub mod sanitize;

#[cfg(test)]
mod tests;

pub use classify::{from_adapter_error, from_anr, from_crash, from_message, from_problem};
pub use fingerprint::{Added, Dedupe, Entry};
pub use model::{Category, Diagnostic, Origin, Severity};
pub use sanitize::{sanitize, sanitize_diagnostic};

pub use crash::{
    collect_crash_buffer, parse_anr, parse_crash_buffer, AnrEvent, CrashEvent, CrashKind,
};
