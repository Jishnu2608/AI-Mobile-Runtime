# Security review — release 1.0 candidate (REL-003)

**Date:** 2026-11-13 · **Scope:** everything shipped in the installer (Runtime Core, `umr` CLI, desktop app, elevation helper), the update path, the MCP server for agents, and third-party dependencies. **Method:** the threat model (T1–T12, master spec §11b) checked against the code, a dependency vulnerability scan (OSV database: 544 crates from `Cargo.lock`, 134 npm packages from the UI lockfile), and the existing test suites.

**Result:** no open high or critical findings. One critical and one high advisory (in the `zip` crate) and one medium advisory (bundled SQLite) were fixed during the review; the remaining items are medium or low and are listed with an owner decision or a follow-up task.

## Threats

| # | Threat | Controls in the code | Verified by | Status |
|---|---|---|---|---|
| T1 | A malicious or confused agent | No shell or file tool over MCP; per-client scopes (agents never get `admin`); running a new project folder or switching the device needs the user's approval (preview banner or `umr approvals`), "always" records a grant; every agent call is audited with sanitised parameters | `agent_api` tests (gate, approvals, grants, audit), live Claude Code run (all calls audited) | OK. Residual: an agent allowed to run shell commands can run `umr` itself (DEC-030); no rate limit on agent calls (low) |
| T2 | Another local process uses the Core API | Named pipe with an owner-only DACL, remote clients rejected, first-instance flag against name squatting; every method except `ping` / `login` needs a token; admin token in an owner-only file, recreated at each Core start; client tokens stored as SHA-256 only | `secure_pipe` / `api_pipe` tests | OK |
| T3 | A web page attacks the local preview server | Loopback only, random port, single-use tokens from `preview.open`, Origin allowlist; no HTTP control API | `umr-stream` tests (tokens single-use, origins) | OK |
| T4 | Command injection | Host processes start with argument arrays (Rust std escapes batch-file arguments since 1.77); on the device, package names and components are validated, typed text is escaped character by character, numbers are formatted, never pasted user text | code review of every `adb shell` format string; `ui::shell_text` tests | OK |
| T5 | Tampered downloads | Official HTTPS hosts only (redirects checked), SHA-256/SHA-1 from the publisher checked before anything is extracted or run; self-update: HTTPS manifest, size + SHA-256 of the installer, optional Authenticode requirement | `verify` / `http` / `self_update` tests | OK. Residual: publisher signatures (INS-009) and a signed release manifest (Phase 9, W8) not yet checked (medium) |
| T6 | A malicious project runs code on build | Workspace trust before any build; agents cannot trust a folder (admin) | `project_api` tests | OK |
| T7 | Secrets leak to agents or logs | Sanitiser on every `diagnostics.*` / `logs.tail` answer and on audit details; diagnostic text in the Core log is now sanitised too; tokens never logged | `sanitize` tests (18 secret formats), audit test (token absent) | OK after fix. Residual: raw tool output files under `%USERPROFILE%\.umr\logs` are not sanitised (they stay local, user-only) (low) |
| T8 | The elevation helper is abused | Fixed set of two actions, no free arguments, UAC on every run, refuses unknown actions | `api_pipe` test (bad actions refused) | OK |
| T9 | Malicious archives (zip-slip, links, bombs) | Raw entry names checked before the library's own sanitising (absolute, drive, `..`), links refused, total size cap, **each entry now capped at its declared size** | `verify` tests (zip-slip cases incl. `C:/…`) | Fixed during review (see below) |
| T10 | Temp-file races | Staging under the managed root; update downloads now go to a fresh per-run folder | review | OK after fix |
| T11 | Telemetry / privacy | No product telemetry; Android CLI always run with `--no-metrics`; the desktop app loads nothing remote (CSP) | review, `tauri.conf.json` CSP | OK |
| T12 | Port hijacking | Port allocator checks that a port is free; Metro on loopback with `adb reverse`; Flutter VM service URL taken from the app's own log | supervisor / adapter tests | OK |

## Dependency scan (OSV, 2026-11-13)

| Package | Advisory | Exposure | Action |
|---|---|---|---|
| `zip` 2.6.1 | GHSA-m665-v7pm-7grj (critical, parser differential), GHSA-q7v9-x93c-xqv3 (high), RUSTSEC-2026-0367/0368/0369/0429, GHSA-8hxf-qr8p-62x4 | Low in practice (archives are checksum-verified first; links refused; `extract()` never called) | **Upgraded to 9.5.1**; added the raw-name check and the per-entry size cap |
| `libsqlite3-sys` 0.30.1 | RUSTSEC-2026-0459 (bundled SQLite CVE, local, user interaction) | The Core's own database in the user profile | **Upgraded `rusqlite` 0.32 → 0.42** (`libsqlite3-sys` 0.40) |
| `glib` 0.18.5 | GHSA-wrw7-89jp-8q8g (moderate) | Linux-only Tauri dependency; not in the Windows binaries | none |
| `proc-macro-error` 1.0.4 | RUSTSEC-2024-0370 (unmaintained) | Build-time macro only | none |
| npm (134 packages) | none | — | — |

Re-run before each release: `python -I scripts/dependency-audit.py .` (queries osv.dev with package names and versions only). After the upgrades it reports only the two non-shipped items above (`glib`, `proc-macro-error`).

## Open items (none high or critical)
- **Unsigned binaries** until a code-signing certificate exists (REL-001, owner): SmartScreen warns and users cannot verify the publisher. Medium.
- **Agent shell bypass** (DEC-030). Medium, accepted until decided.
- **Release manifest not signed** (Phase 9, W8): an attacker controlling the manifest host could point to a different installer with its own checksum. Medium; mitigated by HTTPS and, once signing exists, `require_signed`.
- **Publisher signatures for toolchains** (INS-009). Medium.
- **Audit log retention** (STOR-001), **agent call rate limit**, **raw tool logs not sanitised**. Low.
- **Flaky test** `ports::leases_are_unique_and_released_on_drop` (depends on free ports on the machine; TEST-005). Not a security issue.
