# AI Mobile Runtime

A local desktop runtime that lets you **see and test your Android app while an AI coding agent writes it**.

You open a mobile project in Claude Code, Codex or another coding agent, run one command, and a phone preview opens beside your agent. The runtime prepares the Android toolchain for you, boots an emulator, builds and launches the app, and keeps it updated as the agent changes code. Build errors, crashes and screenshots are made available to the agent, so it can see what you see.

> **Status: early development — Phase 0 (research and proof-of-concept).**
> There is no installable product yet. This repository currently contains throwaway experiments and their measured results.

---

## What it is — and what it is not

**It is**
- a mobile **runtime orchestrator**: environment setup, emulator management, build/launch/reload;
- a **live preview** of the real running app (not a mock rendering), with mouse and keyboard input;
- a **diagnostics** collector that separates app errors from environment errors;
- an **agent integration layer** (CLI and MCP) that works with any coding agent.

**It is not**
- an IDE or code editor;
- an AI coding agent or chatbot;
- a replacement for Android Studio, VS Code, Claude Code or Codex;
- a custom emulator (it uses Google's official emulator and tools).

## Planned user experience

```bash
# inside your Kotlin / Java / Flutter / React Native project
umr run
```

1. Detects the framework and the versions your project needs.
2. Checks your machine and shows a single install plan (JDK, Android SDK, emulator, Flutter, Node — only what is missing), with licences for you to accept.
3. Installs into an isolated, app-managed folder. Your system `PATH`, `JAVA_HOME` and `ANDROID_HOME` are never changed.
4. Boots a phone of the size you choose (compact, standard, large, foldable, flip, tablet).
5. Builds and launches the app, and opens the preview window docked beside your coding agent.
6. Hot reload (Flutter, React Native) or automatic rebuild (Kotlin, Java) as the agent edits code.

## Scope

| Area | Initial release |
|---|---|
| Host OS | Windows 11 x64 (macOS and Linux hosts later) |
| Target | Android emulator (physical devices later) |
| Frameworks | Kotlin, Java, Flutter (Dart), React Native, Expo |
| Agents | Claude Code, Codex (any MCP-capable agent later) |
| iOS | **Out of scope** — requires macOS, Xcode and Apple licences |

## Architecture (planned)

```
 coding agent ──(umr CLI / MCP)──┐
                                 ▼
 desktop preview window ◀──▶ Runtime Core (Rust background service)
 (Tauri 2 + React)           • environment detection and installer
                             • session, process and device management
                             • diagnostics and video streaming
                                 │
               framework adapters (Flutter · React Native · Gradle)
               device provider (Android emulator via Android CLI, adb)
```

- **Runtime Core** owns all state and keeps sessions alive when the agent or window closes.
- **CLI, desktop app and MCP server** are thin clients over a local, user-only named pipe.
- **SDKs are never bundled**; official packages are downloaded on demand and verified.
- Everything runs **locally**. No source code, logs or app data leave your machine.

## Proof-of-concept results so far

| Experiment | Result | Report |
|---|---|---|
| POC-000 — existing SDK inventory | Done | [`poc/reports/POC-000-sdk-inventory.md`](poc/reports/POC-000-sdk-inventory.md) |
| POC-001 — isolated JDK + Android SDK provisioning, emulator boot | Done (part A). Cold boot 39 s, snapshot boot 5 s. `sdkmanager` is deprecated; Android CLI is the SDK backend | [`poc/reports/POC-001-provisioning.md`](poc/reports/POC-001-provisioning.md) |
| POC-003 — live preview in a Tauri window (scrcpy → Rust → WebSocket → WebCodecs) | Works. p95 latency 149 ms at 1280 px. Frame rate limited by the emulator's software video encoder | [`poc/reports/POC-003-preview.md`](poc/reports/POC-003-preview.md) |

Next experiments: host-side emulator frame stream (higher frame rate), framework adapters (Flutter hot reload, Metro/Expo), diagnostics extraction, MCP loop with Claude Code and Codex.

## Repository layout

```
poc/
  preview/        POC-003 throwaway code (Rust bridge, Tauri 2 viewer, WebCodecs page, sweep script)
  reports/        Measured results of each experiment, licence texts reviewed during POC-001
```

Product code (Phase 1 onwards) will follow this layout:

```
apps/desktop      Tauri desktop shell + React UI
apps/cli          umr command (incl. umr mcp)
crates/           Rust Runtime Core components
adapters/         flutter, react-native, gradle-android
providers/        android-emulator
helpers/          gradle-probe, elevated-helper
knowledge-pack/   version compatibility rules
tests/            fixtures, failure corpus, environment matrix, performance
```

## Running the preview proof-of-concept (developers)

Requirements: Windows 11, Rust (MSVC toolchain + Visual Studio Build Tools), an Android SDK with a booted emulator, and `scrcpy-server` v5.0.1.

```powershell
cd poc/preview
cargo build --release

# emulator must already be running, e.g.
#   emulator -avd <name> -no-window -no-audio -no-boot-anim -gpu host

$env:ADB = "<path>\platform-tools\adb.exe"
$env:SCRCPY_SERVER = "<path>\scrcpy-server-v5.0.1"
$env:WEB_DIR = "$PWD\web"
.\target\release\bridge.exe          # terminal 1
.\target\release\viewer.exe          # terminal 2 (add --auto to run the latency/fps measurement)
```

This code is an experiment, not the product. It will be replaced by the Runtime Core in Phase 1.

## Third-party components

- [scrcpy](https://github.com/Genymobile/scrcpy) server (Apache-2.0) — downloaded at a pinned version with SHA-256 verification, not vendored.
- Android SDK components, emulator and Android CLI — downloaded from Google under the Android SDK License Agreement, which the user must accept. Not redistributed.
- Eclipse Temurin JDK (GPLv2 + Classpath Exception) — downloaded on demand.

## Licence

Not yet decided. All rights reserved until a licence is added.
