# Toolchain Universal Mobile Runtime for Antigravity

Install: `agy plugin install https://github.com/Jishnu2608/TUMR` is for repositories whose root is
the plugin; for this folder use `agy plugin install <path to plugins/antigravity>`.

Run your Android app (Kotlin/Java, Flutter, React Native/Expo) on an emulator next to Antigravity:

- The phone opens in its own window on the left of the screen and updates live as the agent edits
  the code (Flutter hot reload, React Native Fast Refresh, an incremental Gradle rebuild for
  Kotlin/Java).
- The agent gets the runtime's MCP tools (`runtime_status`, `get_diagnostic`, `screenshot`, `tap`,
  `wait_for_app` and more).
- When the agent finishes a task and the app still has a build error, crash or runtime error, the
  runtime hands it back to the agent, which keeps fixing until the app is clean. A long Gradle build
  is waited for first. If the same problem comes back five times in a row, the loop stops and the
  runtime's home screen says so.

## Requirements

- Windows 10 or 11 (x64).
- Toolchain Universal Mobile Runtime installed and opened once:
  https://github.com/Jishnu2608/TUMR/releases. The plugin finds it in its default folder
  (`%LOCALAPPDATA%\Toolchain Universal Mobile Runtime`), through the `TUMR_EXE` environment
  variable, or on `PATH`.

On first use the plugin connects Antigravity to the runtime by itself; the runtime's home screen shows
"Antigravity connected itself through its plugin". Disconnect it on the runtime's Agents page.

Licence: Apache-2.0. Name and logo: see TRADEMARKS.md in the repository.
