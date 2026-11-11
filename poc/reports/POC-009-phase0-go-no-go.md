# POC-009 — Phase 0 go/no-go report

**Date:** 2026-11-11 · **Scope:** all Phase 0 experiments · **Host:** Windows 11, Intel Core Ultra 7 258V, Intel Arc 140V, ~31.5 GB RAM
**Recommendation:** **GO for Phase 1**, with the conditions listed in section 5.

## 1. Gate criteria
The Phase 0 gate requires the three riskiest assumptions to hold: provisioning from scratch (PoC-1), embedded preview performance (PoC-3, 12, 13) and framework adapters (PoC-6).

| Gate | Question | Result | Evidence |
|---|---|---|---|
| PoC-1 | Can we provision JDK + Android SDK + emulator into an isolated folder, without Android Studio and without changing system settings? | **Pass** | Verified downloads; boot cold 39 s / snapshot 4–9 s; no change to `PATH`, `JAVA_HOME`, `ANDROID_HOME`; WHPX needed no admin action on this machine. `POC-001-provisioning.md` |
| PoC-3/12/13 | Can the running phone be shown inside our Tauri window fast enough? | **Pass** | Emulator gRPC frames → Rust → WebSocket → WebView2: 36–51 fps; latency p50 52–68 ms, p95 75–107 ms (target p50 ≤ 80, p95 ≤ 150); our pipeline adds ~2–15 ms. `POC-003`, `POC-012`, `POC-013` (with correction) |
| PoC-6 | Can Kotlin/Java, Flutter and React Native/Expo projects build, run and live-update from that toolchain? | **Pass** | Kotlin edit→running 7.1–7.6 s; Flutter hot reload ~0.3 s; Expo Fast Refresh 0.24–0.32 s; Android CLI layout/tap/capture work on all. `POC-006-adapters.md` |

No gate failed. No result requires changing the product concept.

## 2. What Phase 0 changed in the architecture
| Area | Before Phase 0 | After Phase 0 (decision) |
|---|---|---|
| SDK backend | sdkmanager default, Android CLI optional | **Android CLI** (sdkmanager deprecated in cmdline-tools 23.0) — DEC-005 |
| Emulator preview | scrcpy H.264 + WebCodecs | **Emulator gRPC `streamScreenshot`** at window size; scrcpy only for future physical devices — DEC-007 |
| Render + input | undecided | **WebGL2** + gRPC `streamInputEvent` — DEC-008 |
| Latency target | p50 ≤ 80 / p95 ≤ 150 ms | Kept (DEC-010), plus pipeline budget ≤ 20 ms and built-in self-test |
| AVD defaults | not specified | `google_apis` image, ≥ 4 GB RAM, `-gpu host`, own hardware definitions, snapshot after warm-up — DEC-006 |
| Desktop shell | Tauri 2 (owner choice) | Confirmed fit: WebView2 decoding/drawing is not a bottleneck |
| Adapter split | assumed | Confirmed: Android CLI = device/SDK layer; our adapters = Flutter tool, Metro/Expo CLI, Gradle |

## 3. Risks found and how Phase 1–3 must handle them
| # | Finding | Handling (task) |
|---|---|---|
| R1 | Android CLI accepts SDK licences silently | Our UI shows and records licences before any install (master_build §6) |
| R2 | JVM tools crash with "Unable to establish loopback connection" | Set `JAVA_TOOL_OPTIONS=-Djdk.net.unixdomain.tmpdir=<managed>\tmp` for all JVM children (ENV-009) |
| R3 | Builds install SDK packages on their own (platform, NDK, CMake); AGP cannot install NDK any more | Resolver pre-computes and installs them inside the approved plan (RES-005) |
| R4 | Snapshot from a different/broken GPU session renders black app windows | Render health check after every boot; GPU mode stored per snapshot; cold boot on mismatch (DEV-009) |
| R5 | Headless emulator defaults to software GPU; Play Store image + 2 GB RAM swaps | AVD defaults (DEC-006) |
| R6 | `android emulator start` hangs; `emulator create` downloads its own image | Supervisor launches `emulator.exe` directly; own AVD definitions (master_build §6) |
| R7 | Isolation leaks: `%USERPROFILE%\.android`, `%TEMP%\netsimd`, emulator discovery files, `%APPDATA%\.flutter_settings`, `%USERPROFILE%\.expo`, build logs in `%TEMP%` | ENV-008 (Phase 1–2) |
| R8 | Metro binds the LAN IP by default; `CI=1` disables Fast Refresh | `--localhost` + `adb reverse`; never set CI interactively (ADP-007) |
| R9 | First builds are long (Kotlin 3.7 min, Flutter ~3 min, React Native native 22 min) and the toolchain is large (34 GB in this PoC, incl. extras) | Onboarding sets expectations; Expo Go fast path; disk plan shows real sizes |
| R10 | `adb screencap` and Android CLI capture depend on guest render state | Agent screenshots come from the emulator gRPC path (host side) |

## 4. Not validated in Phase 0 (deliberately moved to later phases)
- Virtualization detection and guided enablement on machines where WHPX is off (ENV-007, Phase 1).
- A clean Windows VM run (TEST-001, Phase 1). This PoC used an isolated folder on a developer machine.
- adb coexistence with a running Android Studio (DEV-007, Phase 2).
- Foldable/flip profiles (DEV-008, Phase 2).
- Structured error extraction for all frameworks (DIA-006, Phase 5).
- MCP loop with Claude Code and Codex (MCP-006, Phase 6).
- Android CLI licence/redistribution terms and telemetry behaviour (INS-007).

None of these is expected to block the product; each has a fallback (guided manual steps, separate adb port, stock profiles, log parsing, CLI-only agent access).

## 5. Recommendation and conditions
**GO for Phase 1 (Foundation + provisioning).** Conditions:
1. Owner answers DEC-003 before Phase 1 code starts: Q5 (Windows-only MVP confirmed?) and Q10 (Windows 10 support?).
2. Phase 1 must include R1, R2, R3 and the clean-VM test (TEST-001) in its acceptance criteria.
3. PoC code is not copied as-is. Ported pieces (gRPC message definitions, frame/input bridge logic, WebGL2 renderer, Flutter machine-mode driver) are rewritten to product standards with tests.
4. On approval, these PROVISIONAL decisions become CONFIRMED: Rust Runtime Core, React + TypeScript UI, SQLite + TOML storage, MCP over stdio shim + named pipe JSON-RPC, Temurin JDK source, repository structure.
