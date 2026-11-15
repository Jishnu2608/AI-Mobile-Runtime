# AI Mobile Runtime for VS Code

Run your Android app on an emulator without leaving VS Code — Kotlin, Java, Flutter (Dart) and React Native / Expo, with one set of commands.

- **Run on Emulator** — builds, boots the emulator, installs and launches the app in the open folder. The first run asks to trust the folder (building runs the project's own scripts) and shows its progress in the terminal.
- **Mobile Preview** — the live phone screen in a panel beside your editor: a phone body with a punch-hole camera, click and type like on a phone, rotate, screenshot, problems and logs, and an in-display fingerprint sensor on the lock screen or when your app asks for a fingerprint.
- **Saved files apply themselves** — Flutter hot reload, React Native Fast Refresh, an incremental Gradle rebuild for Kotlin and Java.
- **Problems panel** — build errors, runtime errors and crashes of the running app, with file and line.
- **Copilot agent mode** — *Connect Copilot* gives VS Code's agent the runtime's tools (status, problems, screenshot, tap, reload, fingerprint) through MCP.

The status bar shows what runs; click it for all actions. Commands are under **AI Mobile Runtime** in the Command Palette.

## Requirements

- Windows 11 x64 with the AI Mobile Runtime installed (`umr.exe`). The extension finds it on `PATH` or in `%LOCALAPPDATA%\AI Mobile Runtime`; otherwise set **umr.path** or run **AI Mobile Runtime: Locate umr.exe**.
- The project's toolchain installed through the runtime (`umr plan` / `umr install`, or the desktop app's setup wizard).

## Settings

| Setting | Default | |
|---|---|---|
| `umr.path` | empty | Full path of `umr.exe` |
| `umr.openPreviewOnRun` | `true` | Open the Mobile Preview panel when a run starts |
| `umr.problems` | `true` | Show the runtime's problems in the Problems panel |

## How it works

The extension starts `umr rpc --stdio` and talks to the local Runtime Core through it; only preview, run, reload, stop and problem calls pass (no installs, no trust, no agent administration). The preview frames come from the Core's local socket on `127.0.0.1` with a single-use token. Nothing leaves your PC.

## Licence

Not decided yet (see the project's repository); all rights reserved until then.
