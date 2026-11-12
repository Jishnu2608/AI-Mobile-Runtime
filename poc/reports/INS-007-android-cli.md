# INS-007 — Android CLI behaviour (folded from POC-011)

**Date:** 2026-11-12 · **Machine:** development machine, Windows 11 26300 · **Android CLI:** 1.1.16717390 (in cmdline-tools 23.0)
**Method:** the product's real install (`umr install`, isolated environment, `--no-metrics`) plus read-only probes with the same environment.

## Findings

| # | Question | Result | Product decision |
|---|---|---|---|
| 1 | Telemetry with `--no-metrics` | The product install wrote **no** `cli\analytics\` folder and no metrics spool. A run *without* `--no-metrics` and without the isolated environment (an accidental probe during step 1.8) did create `cli\analytics\metrics\spool\*.bproto` and `last_upload_time` in `%USERPROFILE%\.android`. Whether that file was uploaded is unknown. | Every Android CLI call passes `--no-metrics` (fixed in `txn::android_cli_args`). After each call the installer deletes any spool files (`manage::purge_cli_metrics`) and logs it in the audit log (`android-cli.metrics-purged`). |
| 2 | Self-update | `cmdline-tools\latest\bin\android.exe` is a launcher. On first use it downloads `android-cli.exe` (86 MB) and a bundle with its own Java runtime (147 MB) into `ANDROID_USER_HOME\bin` and `\cli\bundles\<hash>` from `https://dl.google.com/android/cli/latest/...` — the **latest** version, not pinned, and not verified by us. `cli\last_update_check` records a periodic update check. `android update [--url=...]` updates explicitly. The launcher honours `ANDROID_CLI_DOWNLOAD_URL`. | The runtime is recorded as component `android/cli-runtime` with its SHA-256 and bundle id after the first SDK install (`manage::record_cli_runtime`). `umr components` reports a note when the file changes (self-update). Pinning (serving a verified copy through `ANDROID_CLI_DOWNLOAD_URL`) needs a decision: see DEC-012. |
| 3 | Where it writes | With the isolated environment everything stays under the managed root: `android-user-home\bin`, `\cli` (233 MB), `tmp\`. Nothing new in `%USERPROFILE%\.android`. | No change. |
| 4 | `android emulator start` never returns | Not re-tested: the product never uses it. Phase 2 starts `emulator.exe` directly through the Supervisor (POC-001 finding 5). | Re-check in DEV-00x if Android CLI is ever used for emulator start. |
| 5 | Licence prompt | `android sdk install` shows the Terms of Service URL but asks nothing and writes `licenses\android-sdk-license` itself. | Unchanged: the product's own licence gate (INS-004) runs first. |

## Open
- **DEC-012 (owner):** pin the Android CLI runtime by mirroring a verified copy and setting `ANDROID_CLI_DOWNLOAD_URL`, or accept "latest from dl.google.com" with change detection (current behaviour).
- Upload behaviour of `last_upload_time` with metrics on is still unverified (would need network capture); irrelevant while `--no-metrics` is always passed and spool files are purged.
