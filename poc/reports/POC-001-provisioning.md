# POC-001 — Android provisioning on this machine (isolated)

**Date:** 2026-11-11 · **Environment:** this machine, isolated (DEC-004) · **Managed root:** `D:\UMR-poc\`
**Status:** Part A (provisioning + boot) **done**. Part B (framework coverage checklist) **not started** — needs Flutter, Node and sample projects (overlaps POC-006).

## Steps and measurements
| Step | Tool | Result | Time |
|---|---|---|---|
| JDK Temurin 17.0.21 metadata | Adoptium API | ok, SHA-256 published | — |
| cmdline-tools 23.0 metadata | `repository2-3.xml` | ok, SHA-1 published | — |
| Download JDK (182 MB) + cmdline-tools (155 MB) | Invoke-WebRequest | both checksums verified | 136 s |
| Extract into managed root | Expand-Archive | ok | — |
| `sdkmanager --version` | cmdline-tools 23.0 | **deprecated**, forwards to Android CLI | — |
| `android sdk install platform-tools` | Android CLI 1.1.16717390 | exit 0 | 11 s |
| `android sdk install emulator` | | exit 0 (37.2.13) | 241 s |
| `android sdk install platforms/android-36` | | exit 0 | 90 s |
| `android sdk install build-tools/36.0.0` | | exit 0 | 29 s |
| `android sdk install system-images/android-36/google_apis/x86_64` | | exit 0 | 830 s |
| `android emulator create medium_phone` | | exit 0, but downloaded **another** image (`google_apis_playstore`) silently | ~10 min |
| `android emulator start --headless medium_phone` | | device booted, **command never returned** (killed after 10 min and 6.7 min, with private and default adb port) | — |
| `emulator.exe -no-window` cold boot (`-no-snapshot-load`) | emulator 37.2.13 | `sys.boot_completed=1` | **39 s** |
| Snapshot boot | | `sys.boot_completed=1` | **5 s** |
| Device | | Android 16, 1080×2400, 420 dpi, 2 GB guest RAM, qemu ≈ 3.3 GB host RAM | — |

Total managed root after test: **13.47 GB** (includes two system images and download archives).

## Findings
1. **sdkmanager is deprecated.** cmdline-tools 23.0 bundles `android.exe` (Android CLI) and `sdkmanager` forwards to it. The "sdkmanager vs Android CLI" choice is effectively decided by Google: Android CLI is the official path. → Recommend Android CLI as default SDK backend (needs owner approval to change `master_build.md` §6).
2. **Licences are accepted silently.** `android sdk install` has no licence prompt or flag and writes `licenses/android-sdk-license` itself. → Our product must show licence texts and record acceptance **before** calling Android CLI. Licence texts for the packages: `android-sdk-license`, `android-sdk-preview-license` (saved in this folder).
3. **Loopback crash.** Android CLI's Java HTTP client fails with `Unable to establish loopback connection` when the Unix-domain-socket temp dir is not usable. Fix: `JAVA_TOOL_OPTIONS=-Djdk.net.unixdomain.tmpdir=<managed>\tmp`. Product must set this.
4. **`emulator create` picks its own image** (Play Store variant, latest for the API) and downloads it without asking, with no progress when output is piped. → Product must pre-install the chosen image, or create AVDs with explicit hardware config instead of `android emulator create <profile>`.
5. **`emulator start` does not return** in this environment although the device boots. Root cause unknown (sandbox or CLI bug). → Product launches `emulator.exe` directly through the Supervisor and polls `sys.boot_completed`. Keep investigating in POC-007.
6. **Self-updating launcher.** On first run Android CLI downloaded `android-cli.exe`, a bundled Java 25 runtime and `main.jar` into `ANDROID_USER_HOME\bin` and `\cli`. Code downloaded outside our verification. Needs to be documented and, if possible, pinned (check `android update` / offline behaviour).
7. **Telemetry state.** `analytics.settings` shows `hasOptedIn:false` with `--no-metrics`. Crash files are still written locally to `cli\analytics\*.crash` and a `last_upload_time` is recorded. Whether anything is uploaded is **unverified**.
8. **Isolation leaks.**
   - `%USERPROFILE%\.android\` was created (adbkey, emulator flags, userid) at 02:27:58, during `android emulator start`, despite `ANDROID_USER_HOME`. It did not exist before (POC-000). Some child process (adb or emulator) ignores `ANDROID_USER_HOME`. → Product must set `ANDROID_EMULATOR_HOME` and test adb key location (`ADB_VENDOR_KEYS` / HOME) — POC-007.
   - `netsimd` writes artefacts to `%TEMP%\netsimd`.
   - No change to user/system `PATH`, `JAVA_HOME`, `ANDROID_HOME` (verified).
9. **Built-in profiles are generic:** `small_phone`, `medium_phone`, `medium_tablet`, three desktops. No large phone, fold, flip. → Device catalogue needs custom hardware definitions (POC-008).
10. **Acceleration:** WHPX worked without any admin action on this machine.

## Part B — framework coverage checklist (pending)
All items in `todo.md` POC-001 checklist remain open. They need Flutter SDK, Node, and sample Kotlin/Flutter/RN/Expo projects, which belong to POC-006 provisioning. Recommend running Part B together with POC-006.

## Cleanup state
- Emulator, adb and Android CLI processes stopped.
- `D:\UMR-poc\` kept for POC-003/006 (13.47 GB).
- `%USERPROFILE%\.android\` and `%TEMP%\netsimd` left in place; owner to decide on deletion.
