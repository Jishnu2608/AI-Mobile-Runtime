# AI Mobile Runtime

A local desktop runtime that lets you **see and test your Android app while an AI coding agent writes it**.

Open a mobile project, run one command, and a phone preview docks beside your coding agent. The runtime prepares the Android toolchain, boots an emulator, builds and launches the app, and keeps it updated as the code changes. Your agent (Claude Code, Codex or any MCP client) gets the same view you have: the screen, the errors with file and line, and the controls.

> **Status: 1.0 release candidate** (Phases 1–6 and 8 built; see [Status](#status)). Windows 11 x64 and Android only.

```powershell
umr trust .                      # once per project folder
umr run                          # toolchain check → emulator → build → app in the docked preview
umr agent setup claude-code      # or codex: connect your agent (shows the changes, asks first)
```

## What it does
- **Zero-config toolchain** — detects what your project needs (JDK, Android SDK packages, emulator image, Flutter, Node) and installs only that, from the official publishers, verified, into its own folder. Your `PATH`, `JAVA_HOME`, `ANDROID_HOME` and Android Studio are never touched.
- **Live preview beside your agent** — the real app on a real emulator, 40+ fps, mouse and keyboard input, phone / foldable / tablet sizes, hot reload for Flutter and React Native, rebuilds for Kotlin/Java.
- **Diagnostics an agent can use** — build errors, crashes, ANRs and runtime exceptions, classified (build, runtime, crash, environment, device, network), deduplicated, located at file and line, and marked as *your code* or *environment*.
- **Agent integration** — an MCP server with 16 tools (status digest, problems, logs, screenshot, UI tree, input, reload, run) plus a CLI; per-agent permissions, approvals for new folders and device switches, an audit log, and sanitised output.

It is **not** an IDE, an AI agent or an emulator of its own: it orchestrates Google's official tools and the frameworks' own CLIs.

## Install
Download the installer (`AI-Mobile-Runtime_<version>_x64-setup.exe`), check its SHA-256, and run it — per user, no administrator rights. A portable ZIP and a CLI-only ZIP are also published. Requirements and the full walk-through: **[docs/user](docs/user/README.md)**.

| Guide | |
|---|---|
| [Getting started](docs/user/getting-started.md) | install → setup wizard → first app → connect an agent |
| [Requirements](docs/user/requirements.md) | Windows 11 x64, virtualization, 16 GB RAM recommended, 8–15 GB disk |
| [Agents](docs/user/agents.md) | Claude Code, Codex, other MCP clients; tools and permissions |
| [Troubleshooting](docs/user/troubleshooting.md) · [Privacy and security](docs/user/privacy-and-security.md) · [Uninstall](docs/user/uninstall-and-data.md) · [Command reference](docs/user/cli-reference.md) | |

## Scope
| Area | 1.0 |
|---|---|
| Host | Windows 11 x64 (macOS / Linux later) |
| Target | Android emulator (physical devices later) |
| Frameworks | Kotlin, Java, Flutter, React Native, Expo |
| Agents | Claude Code, Codex, any MCP client |
| iOS | Out of scope (needs macOS, Xcode and Apple's licences) |

## Architecture
```
 coding agent ──(MCP: umr mcp · CLI: umr)──┐
                                           ▼
 desktop preview window ◀──────▶ Runtime Core (Rust background service, per user)
 (Tauri 2 + React, WebGL2)       • environment scan, install plans, verified installer
                                 • devices, sessions, supervised processes
                                 • framework adapters: Gradle · Flutter · React Native/Expo
                                 • diagnostics (classifier, dedupe, sanitiser)
                                 • preview stream (emulator gRPC frames → WebSocket)
                                 • agent permissions, approvals, audit
```
The CLI, the desktop app and the MCP server are thin clients of the Core over a user-only named pipe. Sessions survive closing the window or the agent. Everything runs locally; nothing about your code or app leaves your machine.

## Status
| Phase | | |
|---|---|---|
| 1 Foundation + provisioning | ✅ | environment scan, Knowledge Pack, plans, verified transactional installer, setup wizard |
| 2 Android runtime | ✅ | private adb server, device catalogue, emulator boot + health, supervisor, sessions |
| 3 Framework adapters | ✅ | Kotlin/Java (Gradle), Flutter, React Native / Expo, file watching |
| 4 Preview | ✅ | docked window, 40–42 fps, our pipeline 12–17 ms p95 |
| 5 Diagnostics | ✅ | 76-case failure corpus: category 100 %, file:line 96.6 % |
| 6 Agents | ✅ | MCP server; Claude Code fixed a seeded bug through it on its own |
| 8 Release readiness | ✅ except signing and licence | per-user installer (6 MB), updates, security review, docs |
| 9 Website + distribution | planned | |

Open before a public 1.0: the product licence, a code-signing certificate, and the Codex live check.

## Building from source
Windows 11, Rust (MSVC; the version is pinned in `rust-toolchain.toml`), Visual Studio Build Tools, Node.js 20+.

```powershell
cargo build --release                         # Core, CLI, desktop app, helpers
powershell -File scripts\ci-local.ps1         # the same checks as CI: fmt, clippy, tests, release build
powershell -File scripts\release.ps1          # installer, portable ZIP, CLI ZIP, SHA256SUMS, releases.json → dist\
```

| Folder | |
|---|---|
| `crates/` | Runtime Core and its parts (`umr-core`, `-api`, `-storage`, `-env`, `-resolver`, `-installer`, `-supervisor`, `-device`, `-session`, `-stream`, `-diagnostics`, `-mcp`, `-platform`) |
| `adapters/` | adapter contract (`sdk`) and the Gradle, Flutter and React Native adapters |
| `apps/` | `cli` (`umr`), `desktop` (Tauri app + React UI) |
| `helpers/` | `elevated-helper` (the two administrator actions) |
| `knowledge-pack/` | version compatibility rules |
| `tests/` | failure corpus, environment matrix |
| `docs/` | user guide, security review |
| `poc/` | Phase 0 experiments and their measured reports (frozen) |

See [CONTRIBUTING.md](CONTRIBUTING.md) to contribute and [SECURITY.md](SECURITY.md) to report a vulnerability.

## Third-party components
Shipped: the Rust crates and npm packages listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Downloaded at the user's request and never redistributed: Android SDK components, the emulator and Android CLI (Google, Android SDK License Agreement — <https://developer.android.com/studio/terms>, shown in the app for acceptance), Eclipse Temurin JDK (GPLv2 + Classpath Exception), Flutter (BSD-3-Clause), Node.js (MIT).

Android is a trademark of Google LLC. This project is not affiliated with or endorsed by Google.

## Licence
Not decided yet. All rights reserved until a licence is added.
