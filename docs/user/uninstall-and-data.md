# Uninstall and your data

## Where things are
| What | Where |
|---|---|
| The programs | `%LOCALAPPDATA%\Toolchain Universal Mobile Runtime` (installer) or wherever you unpacked the portable ZIP |
| Settings, database, logs, agent tokens | `%USERPROFILE%\.tumr` (`config.toml`, `tumr.db`, `logs\`, `agents\`). Audit entries and scans older than `[storage] retention_days` (default 90) are deleted when the Core starts |
| Toolchains, emulator images, emulators (AVDs), download cache | `[storage] toolchains_dir` in `config.toml`; empty = inside `%USERPROFILE%\.tumr` |
| Agent configuration written by `tumr agent setup` | the project's `.mcp.json` / `.claude\skills\tumr-runtime\`, `AGENTS.md`, `%CODEX_HOME%\config.toml` |
| adb keys | `%USERPROFILE%\.android\adbkey*` — written by Google's adb itself, shared with Android Studio |

## Free space without uninstalling
`tumr components` shows sizes; `tumr remove <id>` removes one component; `tumr clean` deletes the download cache and old logs.

## Uninstall
Windows Settings → Apps → **Toolchain Universal Mobile Runtime** → Uninstall. It stops the Runtime Core and its emulators, takes the install folder off your user `PATH` (if you turned that on) and removes the programs. **Your data is kept** — emulator images and toolchains can be many GB and may be reused later. To remove everything, delete afterwards:
1. `%USERPROFILE%\.tumr`
2. your `toolchains_dir`, if you set one elsewhere
3. the agent entries (`tumr` in `.mcp.json`, the `tumr:begin … tumr:end` section in `AGENTS.md`, `[mcp_servers.tumr]` in Codex's `config.toml`)

Portable copy: run `tumr stop`, then delete its folder and the items above.
