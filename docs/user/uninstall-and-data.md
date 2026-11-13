# Uninstall and your data

## Where things are
| What | Where |
|---|---|
| The programs | `%LOCALAPPDATA%\AI Mobile Runtime` (installer) or wherever you unpacked the portable ZIP |
| Settings, database, logs, agent tokens | `%USERPROFILE%\.umr` (`config.toml`, `umr.db`, `logs\`, `agents\`) |
| Toolchains, emulator images, emulators (AVDs), download cache | `[storage] toolchains_dir` in `config.toml`; empty = inside `%USERPROFILE%\.umr` |
| Agent configuration written by `umr agent setup` | the project's `.mcp.json` / `.claude\skills\umr-runtime\`, `AGENTS.md`, `%CODEX_HOME%\config.toml` |
| adb keys | `%USERPROFILE%\.android\adbkey*` — written by Google's adb itself, shared with Android Studio |

## Free space without uninstalling
`umr components` shows sizes; `umr remove <id>` removes one component; `umr clean` deletes the download cache and old logs.

## Uninstall
Windows Settings → Apps → **AI Mobile Runtime** → Uninstall. It stops the Runtime Core and its emulators and removes the programs. **Your data is kept** — emulator images and toolchains can be many GB and may be reused later. To remove everything, delete afterwards:
1. `%USERPROFILE%\.umr`
2. your `toolchains_dir`, if you set one elsewhere
3. the agent entries (`umr` in `.mcp.json`, the `umr:begin … umr:end` section in `AGENTS.md`, `[mcp_servers.umr]` in Codex's `config.toml`)

Portable copy: run `umr stop`, then delete its folder and the items above.
