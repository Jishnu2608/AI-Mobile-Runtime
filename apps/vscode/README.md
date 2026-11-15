# AI Mobile Runtime for VS Code

**Run your Android app on an emulator without leaving VS Code** — for Kotlin, Java, Flutter (Dart) and React Native / Expo.

AI Mobile Runtime builds your app, boots an Android emulator and shows the running app in a phone frame beside your code. Saved files apply themselves, build errors and crashes land in the Problems panel, and Copilot's agent mode can see the screen and check its own changes.

## Features

**Run on Emulator.** One command from the Command Palette or the status bar checks the toolchain, boots the emulator, builds, installs and launches the app in the open folder. The first run asks you to trust the folder, because building runs the project's own scripts.

**Mobile Preview.** The live phone screen in a panel beside your editor: click and type like on a phone, rotate it, take screenshots, switch between phone, foldable and tablet sizes, and stop the app. On the lock screen, or when your app asks for a fingerprint, an in-display fingerprint sensor appears — hold it to touch the sensor.

**Saved files apply themselves.** Flutter hot reload and React Native Fast Refresh in about a second; an incremental Gradle rebuild for Kotlin and Java.

**Problems where you look for them.** Build errors, runtime errors and crashes of the running app appear in the Problems panel with file and line, next to your compiler's own.

**Copilot agent mode.** *Connect Copilot* gives VS Code's agent the runtime's tools: status, problems, screenshots, taps, reloads and the fingerprint sensor, so it can test what it builds.

## Requirements

- **Windows 11 x64** with CPU virtualization turned on.
- **AI Mobile Runtime** installed on the same PC. It sets up the Android toolchain for your project and runs the emulator. Get it from the [AI Mobile Runtime project](https://github.com/Jishnu2608/AI-Mobile-Runtime).

The extension finds `umr.exe` on `PATH` or in the default install folder. Elsewhere, run **AI Mobile Runtime: Locate umr.exe** or set `umr.path`.

## Getting started

1. Open your app's folder.
2. Click **Mobile** in the status bar, then **Run on Emulator**.
3. The phone appears in the **Mobile Preview** panel. Edit, save and watch it update.

| Command | |
|---|---|
| AI Mobile Runtime: Run on Emulator | Build, install and launch the app in the open folder |
| AI Mobile Runtime: Open Mobile Preview | The live phone beside the editor |
| AI Mobile Runtime: Reload (hot) · Restart the App · Rebuild and Reinstall · Stop the App | Apply changes or stop the app |
| AI Mobile Runtime: Connect Copilot (agent mode) | Give Copilot the runtime's tools |
| AI Mobile Runtime: Locate umr.exe | Point the extension at the runtime |

## Settings

| Setting | Default | |
|---|---|---|
| `umr.path` | empty | Full path of `umr.exe` |
| `umr.openPreviewOnRun` | `true` | Open the Mobile Preview when a run starts |
| `umr.problems` | `true` | Show the runtime's problems in the Problems panel |

## Privacy

Everything runs on your computer. The extension talks only to the AI Mobile Runtime on this PC; the preview streams over a local connection with a single-use token. No telemetry.

## Support

Questions and bug reports: [GitHub issues](https://github.com/Jishnu2608/AI-Mobile-Runtime/issues). Full guide: [VS Code](https://github.com/Jishnu2608/AI-Mobile-Runtime/blob/main/docs/user/vscode.md).
