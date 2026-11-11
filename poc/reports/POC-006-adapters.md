# POC-006 — Framework adapter feasibility (+ POC-001 Part B)

**Date:** 2026-11-11 · **Host:** Windows 11, Intel Core Ultra 7 258V, ~31.5 GB RAM
**Code (throwaway):** `poc/adapters/measure-reload.ps1` (edit → log marker timing), `poc/adapters/flutter-machine.ps1` (drives `flutter run --machine`), `poc/adapters/gpu-check.ps1` (render health check via gRPC screenshot)
**Toolchain (managed root, verified downloads):** Temurin JDK 17.0.21 · Android CLI 1.1 · emulator 37.2.13 · Flutter 3.47.9 / Dart 3.13.7 (SHA-256 verified, 1.85 GB) · Node v26.13.0 LTS (SHA-256 verified against SHASUMS256.txt; GPG signature not checked) · Expo SDK 58 / React Native 0.88.1
**Device:** AVD `medium_phone` (Android 16, google_apis, 4 GB, `-gpu host`)
**Status:** **Pass.** All four framework paths build, install, launch and live-update on the emulator from an isolated toolchain.

## Results
| Framework / path | Project creation | First (cold) build + run | Edit → visible re-render | Mechanism |
|---|---|---|---|---|
| Kotlin (Compose, AGP 9.0.1, Gradle 9.1, compileSdk 37) | `android create` 5 s | Gradle cold **3 m 40 s**; install+launch via `android run --apks` | **7.1–7.6 s** (Gradle 4.5–4.7 s + delta install ~2 s) | Rebuild + reinstall (no hot reload) |
| Flutter 3.47.9 (Android target) | `flutter create` 8 s | **185 s** cold (`flutter run --machine`), after NDK install | Hot reload **307 ms** warm (1.2–2.4 s first two); hot restart **~2.2 s** | `flutter run --machine` JSON-RPC `app.restart` |
| Expo Go (SDK 58) | `create-expo-app` 12 s + `npm install` 185 s | Expo Go auto-downloaded + installed; first bundle 8.2 s | Fast Refresh **251–321 ms** | Metro watcher |
| Expo dev build / React Native native build (RN 0.88.1) | `expo run:android` (prebuild) | **22 m 3 s** cold Gradle (C++ via CMake), bundle 4.8 s | Fast Refresh **239–297 ms** | Metro watcher |

Edit-to-render was measured by changing a `UMR_RENDER <label>` marker printed from a render/build method and timing until the new label appeared in logcat (Flutter: machine-mode `app.log` event).

## Android CLI coverage checklist (POC-001 Part B)
| Check | Result |
|---|---|
| Kotlin/Java: `android build` + install | **Works.** `android build ':app@debug'` 12 s warm (writes build log to `%TEMP%`); `android run --apks` 1.9–2.1 s with delta install. First cold `android build` stalled silently (JVM loopback issue, see finding 3) |
| Flutter: `android run --apks=<flutter apk>` | **Works** (60 MB debug APK, 4.1 s). `android build` on Flutter not tried — Flutter's own tool must drive the build (Dart compile, engine artefacts) |
| React Native / Expo: install + launch | **Works** via `expo run:android` (which uses adb); `android run --apks` path equivalent |
| SDK packages + emulator for all four frameworks | **Works** (`android sdk install`); framework builds also auto-installed packages themselves (finding 5) |
| `android layout` (UI tree) | **Works on Kotlin, React Native and Flutter** (Flutter via semantics: "Increment", counter value). ~4.7–5 s per call (installs a layout instrumentation server first time) |
| `android input tap '<selector>'` | **Works on Flutter** (`desc=Increment`, counter 0 → 2), ~1.6 s per call. Selector syntax `key=value` (`text`, `id`, `class`, `desc`) |
| `android screen capture` | **Works** (1.9 s, full app content on a healthy render session) |
| `android device hub` | Not tested — separate desktop window; the product uses its own embedded preview |
| Hot reload via Android CLI | **None** for any framework (as expected). Flutter tool / Metro required |
| Installs Flutter / Node / package managers | **No** (as expected). Our provisioning required |
| Error output | Android CLI reports Gradle results only; Dart and JS errors come from Flutter machine events / Metro logs |

**Coverage summary:** Android CLI is a good **device-and-SDK layer** for all frameworks (install, launch, layout, input, capture, SDK packages). **Framework adapters remain our job** (Flutter tool, Metro/Expo CLI, provisioning of Flutter/Node), confirming the planned split.

## Findings
1. **Snapshot + GPU mismatch breaks rendering (critical).** Restoring an AVD snapshot taken in a different/broken GPU session under `-gpu host` gives **black app windows** (only the navigation bar renders) while the system reports apps as resumed; every capture path (adb screencap, Android CLI capture, gRPC) shows black. Cold boot in either GPU mode fixes it; a snapshot saved from a healthy `-gpu host` session restores correctly (4 s boot). → Product must: record GPU mode per snapshot, cold-boot on mismatch, and run a **render health check** after every boot (gRPC screenshot of a known screen must not be black).
2. **POC-012/013 preview numbers were affected** (see correction in `POC-013-preview-latency.md`). On a healthy session: latency **p50 52–68 ms, p95 75–107 ms**, 36–51 fps at 540–720 px streams.
3. **JVM loopback fix is needed for every JVM tool**, not only Android CLI: Gradle failed with `java.io.IOException: Unable to establish loopback connection` without `JAVA_TOOL_OPTIONS=-Djdk.net.unixdomain.tmpdir=<managed>\tmp`. The first `android build` stalled silently because of it.
4. **NDK must be provisioned by us.** Flutter's template requires NDK 28.2.13676358; AGP's auto-install failed with "Android sdkmanager did not install NDK" (sdkmanager is deprecated and forwards to Android CLI). `android sdk install ndk/28.2.13676358` fixed it. React Native additionally required NDK 27.1.12297006.
5. **Build tools install SDK packages silently.** During builds, AGP installed `platforms/android-37.0`, `cmake/3.22.1` and NDK 27.1 on its own, relying on the licence file already accepted. → Resolver must pre-compute these from project files (compileSdk, ndkVersion, CMake) and include them in the user-approved plan, so nothing is installed outside the plan.
6. **Android CLI template quirk:** `android create` tried to install `platforms/android-37` (not found; real id `android-37.0`) but still created the project.
7. **Metro defaults to the LAN IP** (`exp://192.168.x.x:8081`); `--localhost` + `adb reverse` works and avoids exposing Metro on the network → product default.
8. **`CI=1` disables Metro watch mode** (no Fast Refresh). Product must not set CI for interactive sessions; Expo Go install worked without prompts once installed.
9. **Cold builds are long on first run** (Kotlin 3.7 min, Flutter ~3 min, React Native native 22 min). Most of this is one-time downloads and C++ compilation. Onboarding should set expectations and warm Gradle caches; Expo Go path gives a working preview in under a minute.
10. **New isolation leaks:** `%APPDATA%\.flutter_settings` (Flutter config, created by `flutter config`), `%USERPROFILE%\.expo` (Expo CLI state), Android CLI build logs in `%TEMP%`. (`%USERPROFILE%\.gradle` pre-existed and was not used.)
11. **Disk:** managed root grew to **34.1 GB** (two system images, two NDKs, CMake, Flutter, Node, Gradle and npm caches, three projects).

## Assessment against POC-006 success criteria
| Criterion | Result |
|---|---|
| Flutter hot reload ≤ 2 s | **Met** (307 ms warm; first reloads 1.2–2.4 s) |
| RN/Expo reload programmatically triggered | **Met** via Metro file watching (Fast Refresh 239–321 ms); explicit reload command not required |
| Native incremental cycle measured | **Measured:** 7.1–7.6 s |

**Verdict: PASS.** Framework adapter model is feasible as designed: Flutter adapter over `flutter run --machine`; React Native adapter over Metro/Expo CLI (Expo Go fast path + dev build); Gradle adapter over Gradle/Android CLI with delta install.

## Cleanup
Metro, Gradle daemons, emulator and adb stopped. Leak folders listed above left in place (owner decision, CLEAN-001).
