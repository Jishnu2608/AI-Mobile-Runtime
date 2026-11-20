# VS Code

The **Toolchain Universal Mobile Runtime** extension runs your Android app on the emulator from VS Code: the live phone in a panel beside your code, build errors and crashes in the Problems panel, and the runtime's tools for Copilot's agent mode. It works the same for Kotlin, Java, Flutter (Dart) and React Native / Expo.

## Install
1. Install the Toolchain Universal Mobile Runtime itself (see [Getting started](getting-started.md)).
2. In VS Code: **Extensions › … › Install from VSIX…** and pick `tumr-vscode.vsix` (built by `npm --prefix apps/vscode run package` into `apps/vscode/dist`), or run `code --install-extension tumr-vscode.vsix`.
3. Open your app's folder. The extension starts when the folder holds an Android, Flutter or React Native project; a **Mobile** item appears in the status bar (bottom left).

It finds `tumr.exe` on `PATH` or in `%LOCALAPPDATA%\Toolchain Universal Mobile Runtime`. Elsewhere: set **tumr.path**, or run **Toolchain Universal Mobile Runtime: Locate tumr.exe**. Inside this repository it also uses the newest `target\release` or `target\debug` build, so the samples work straight from a checkout.

The extension does not run in a workspace VS Code does not trust: running a project builds it with its own scripts.

## Use it
| Command (Command Palette › *Toolchain Universal Mobile Runtime*) | What it does |
|---|---|
| **Run on Emulator** | `tumr run` for the open folder in VS Code's terminal: checks the toolchain, boots the emulator, builds, installs, launches. The first time it asks to trust the folder. Opens the Mobile Preview |
| **Open Mobile Preview** | The live phone beside the editor: click and type like on a phone, rotate, screenshot, reload, problems and logs (☰ / ⚠), the device menu, and the in-display fingerprint sensor on the lock screen |
| **Reload (hot)** · **Restart the App** · **Rebuild and Reinstall** · **Stop the App** | The same as `tumr reload hot / restart / full` and `tumr run --stop` |
| **Connect Copilot (agent mode)** | `tumr agent setup vscode`: writes `.vscode/mcp.json` and the agent instructions (shows the changes first), so Copilot's agent mode can read the status and problems, take screenshots, tap, reload and use the fingerprint sensor |

Saved files apply themselves while the app runs: Flutter hot reload, React Native Fast Refresh, an incremental Gradle rebuild and reinstall for Kotlin and Java (about 15–20 s).

The **Problems** panel lists the running app's build errors, runtime errors and crashes with file and line, marked *Toolchain Universal Mobile Runtime*; problems of the toolchain or the device say *(environment)*. The status bar shows the running app; click it for all actions.

## Settings
| Setting | Default | |
|---|---|---|
| `tumr.path` | empty | Full path of `tumr.exe` |
| `tumr.openPreviewOnRun` | `true` | Open the Mobile Preview when a run starts |
| `tumr.problems` | `true` | Show the runtime's problems in the Problems panel |

## Samples
`sample-app/` in the repository has the same small calculator four times — `sample-kotlin`, `sample-java`, `sample-dart` (Flutter) and `sample-react-native` — each with its own README and unit tests. Open one folder in VS Code and run **Run on Emulator**.

## How it talks to the runtime
The extension starts `tumr rpc --stdio` and sends JSON requests through it to the local Runtime Core. Only preview, run, reload, stop and problem calls pass: no installs, no project trust, no agent administration. The preview frames come from the Core's socket on `127.0.0.1`, opened with a single-use token; the Core accepts it from VS Code's web view and the desktop app only.
