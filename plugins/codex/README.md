# Toolchain Universal Mobile Runtime for Codex

Install: `codex plugin marketplace add Jishnu2608/TUMR`, then install **tumr** from the
marketplace in the Codex app's Plugins directory (or `codex plugin add tumr@tumr`).

Run your Android app (Kotlin/Java, Flutter, React Native/Expo) on an emulator next to Codex:

- The phone opens in its own window on the left of the screen and updates live as the agent edits
  the code (Flutter hot reload, React Native Fast Refresh, an incremental Gradle rebuild for
  Kotlin/Java).
- The agent gets the runtime's MCP tools (`runtime_status`, `get_diagnostic`, `screenshot`, `tap`,
  `wait_for_app` and more).
- When the agent finishes a task and the app still has a build error, crash or runtime error, the
  runtime hands it back to the agent (Codex CLI; trust the plugin's hook once in `/hooks`). In the
  Codex app hooks run under a sandbox account the runtime does not let in, so there the agent checks
  with `runtime_status` and `wait_for_app` instead.

## Requirements

- Windows 10 or 11 (x64).
- Toolchain Universal Mobile Runtime installed and opened once:
  https://github.com/Jishnu2608/TUMR/releases. The plugin finds it in its default folder
  (`%LOCALAPPDATA%\Toolchain Universal Mobile Runtime`), through the `TUMR_EXE` environment
  variable, or on `PATH`.

On first use the plugin connects Codex to the runtime by itself; the runtime's home screen shows
"Codex connected itself through its plugin". Disconnect it on the runtime's Agents page.

Licence: Apache-2.0. Name and logo: see TRADEMARKS.md in the repository.
