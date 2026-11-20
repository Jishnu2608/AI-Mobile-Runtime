# Contributing

Thanks for helping. The project is young; issues with a clear reproduction are as valuable as code.

## Before you start
- **Bugs:** open an issue with the bug template. Attach `tumr doctor --json`, `tumr problems --json` and, if useful, the newest Core log from `%USERPROFILE%\.tumr\logs`. `tumr` output is sanitised already; check raw log files for secrets before sharing them.
- **Diagnostics misses:** a failure the runtime classified wrongly (or not at all) is best reported as a new case for `tests/failure-corpus/` — see its README for the format. Real tool output beats invented output.
- **Features:** open an issue first so we can agree on the approach. The scope is deliberate: Windows + Android, local only, agent-agnostic (see the README's Scope).
- **Security issues:** never in a public issue — see [SECURITY.md](SECURITY.md).

## Development setup
Windows 11, Rust (MSVC; version pinned in `rust-toolchain.toml`), Visual Studio Build Tools, Node.js 20+.

```powershell
cargo build
cargo test --workspace
powershell -File scripts\ci-local.ps1      # what CI runs: fmt, clippy (pedantic, -D warnings), UI build, tests, release build
```
The desktop UI alone: `npm --prefix apps/desktop/ui run dev` (sample data in a browser).

## Repository layout
| Folder | |
|---|---|
| `crates/` | The Runtime Core and its parts (`tumr-core`, `-api`, `-storage`, `-env`, `-resolver`, `-installer`, `-supervisor`, `-device`, `-session`, `-stream`, `-diagnostics`, `-mcp`, `-platform`) |
| `adapters/` | The adapter contract (`sdk`) and the Gradle, Flutter and React Native adapters |
| `apps/` | `cli` (`tumr`), `desktop` (Tauri app with the React UI), `vscode` (the VS Code extension) |
| `helpers/` | `elevated-helper` (the two administrator actions) |
| `knowledge-pack/` | Version compatibility rules |
| `sample-app/` | The calculator sample in Kotlin, Java, Dart and React Native |
| `webapp/frontend/` | The website |
| `tests/` | Failure corpus and environment matrix |
| `docs/` | User guides |

## Rules for changes
1. **Tests with every change.** Unit tests always; where a change touches devices or tools, an integration test (marked `#[ignore]` if it needs an emulator) or a recorded manual check in the pull request.
2. **`scripts\ci-local.ps1` must pass.** Clippy runs with `pedantic` and `-D warnings`; `unsafe` only in `crates/tumr-platform`, each block with a `// SAFETY:` comment.
3. **Never change the user's machine outside the runtime's folders** without an explicit, named consent step. No global `PATH` / `JAVA_HOME` / `ANDROID_HOME` changes, no writes into existing SDKs, no telemetry.
4. **Downloads only from official HTTPS hosts, checksum-verified before use.** New hosts need a reason in the pull request.
5. **Generated files** — `THIRD_PARTY_NOTICES.md` (`scripts\third-party-notices.ps1`) and `docs/user/cli-reference.md` (`scripts\cli-reference.ps1`) — are regenerated, not edited.
6. **Plain English** in code comments, messages and docs: short sentences, one term per thing.
7. One topic per pull request; describe what you checked and how.

## Licence of contributions
The project is licensed under the [Apache License, Version 2.0](LICENSE). Unless you state otherwise, any contribution you submit for inclusion is licensed under the same terms, as section 5 of the licence says, with no additional terms or conditions. For larger changes, open an issue or discussion first.
