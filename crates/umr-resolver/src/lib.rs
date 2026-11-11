//! Resolver (Phase 1, step 1.7).
//!
//! | Module | Task |
//! |---|---|
//! | [`pack`] | RES-001 Knowledge Pack: format, validation, Ed25519 signature check |
//! | `knowledge-pack/pack.json` | RES-002 initial rules, each with an official source URL |
//! | [`project`] | RES-003 project constraint readers (Gradle, AGP, SDK levels, pubspec, package.json, …) |
//! | [`resolve`] | RES-004/005 version resolution incl. NDK, `CMake`, compileSdk packages |
//! | [`plan`] | RES-004 install plan: reuse vs install, sizes, licences, admin actions, explanations |
//!
//! Everything here is pure computation over files and the scan report: nothing is downloaded,
//! installed or changed, and the user's project files are only read.

pub mod pack;
pub mod plan;
pub mod project;
pub mod resolve;
pub mod version;
