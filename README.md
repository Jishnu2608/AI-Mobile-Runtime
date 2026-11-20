<p align="center">
  <img src="brand/logo-mark.svg" width="96" height="96" alt="Toolchain Universal Mobile Runtime logo" />
</p>

<h1 align="center">Toolchain Universal Mobile Runtime</h1>

<p align="center">
  <strong>See and test your Android app while an AI coding agent builds it.</strong><br />
  A live emulator beside your code, errors your agent can act on, and a toolchain that sets itself up.
</p>

<p align="center">
  Windows 11 · Kotlin · Java · Flutter · React Native · Expo · Claude Code · Codex · Cursor · Antigravity · VS Code
</p>

---

## Why Toolchain Universal Mobile Runtime

AI coding agents write mobile code quickly, but they cannot see the app they are building, and setting up Android tooling by hand takes hours. Toolchain Universal Mobile Runtime closes that gap.

Open a project and run it. The runtime installs exactly what the project needs, boots an Android emulator, builds and launches your app, and shows it live in a phone frame beside your editor. Your coding agent gets the same view you have: the screen, the errors with file and line, and the controls to tap, reload and check its own work.

## Highlights

- **Setup from your project.** It reads the project and installs only what is missing — JDK, Android SDK, emulator image, Flutter, Node.js, Git — from the official publishers, each download verified. Your existing tools and settings are never changed.
- **A real phone beside your code.** A live, interactive emulator in a phone frame with a punch-hole camera and an in-display fingerprint sensor. Click and type in it like on a phone, rotate it, switch between phone, foldable and tablet sizes.
- **Every framework, one workflow.** Kotlin, Java, Flutter and React Native / Expo, each built with its own tools. Saved files apply themselves: hot reload for Flutter, Fast Refresh for React Native, a quick rebuild for Kotlin and Java.
- **Problems your agent can act on.** Build errors, runtime errors and crashes are classified, de-duplicated and located at file and line — and marked as your code or the environment.
- **Agents that check their own work.** Claude Code, Codex, Cursor, Antigravity, Claude Desktop and VS Code connect in one command. When an agent finishes, the runtime hands back any error it left, so it fixes it before stopping.
- **Built for VS Code too.** Run on the emulator from the Command Palette, see the phone in a panel beside your code, and find runtime problems in the Problems panel.
- **Private by design.** Everything runs on your computer. No account, no cloud service, no telemetry.

## Get started

1. **Install** — download the installer and run it. It installs for your Windows user, without administrator rights.
2. **Set up** — the setup wizard scans your PC, lets you choose your app's folder, and shows exactly what will be installed before anything downloads.
3. **Run** — one click (or `tumr run`) boots the emulator, builds your app and opens the live preview.
4. **Connect your agent** — one click (or `tumr agent setup claude-code`) gives your coding agent the runtime's tools.

```powershell
tumr run                          # build, launch and preview the app in this folder
tumr agent setup claude-code      # or codex, cursor, claude-desktop, vscode
```

The full walk-through is in the **[getting-started guide](docs/user/getting-started.md)**.

## Requirements

| | |
|---|---|
| Operating system | Windows 11 x64 (build 22000 or later) |
| Processor | Virtualization (VT-x / AMD-V) turned on in the firmware |
| Memory | 16 GB recommended, 8 GB minimum |
| Disk | About 60 MB for the app, plus 8–15 GB for toolchains and one emulator image |

The app tells you if anything is missing and turns on Windows Hypervisor Platform for you when needed. Details: [requirements](docs/user/requirements.md).

## Try it with the samples

[`sample-app/`](sample-app) contains the same small calculator built four ways — [Kotlin](sample-app/sample-kotlin), [Java](sample-app/sample-java), [Dart / Flutter](sample-app/sample-dart) and [React Native](sample-app/sample-react-native). Open one and run it to see the runtime at work.

## Documentation

| Guide | |
|---|---|
| [Getting started](docs/user/getting-started.md) | Install, set up, run your first app and connect an agent |
| [Coding agents](docs/user/agents.md) | Connecting Claude Code, Codex, Cursor, Antigravity and other MCP clients; tools and permissions |
| [VS Code](docs/user/vscode.md) | The extension: run on the emulator, phone preview panel, Problems panel, Copilot |
| [Troubleshooting](docs/user/troubleshooting.md) | Common problems and how to fix them |
| [Privacy and security](docs/user/privacy-and-security.md) | What the runtime does on your computer, and what it never does |
| [Command reference](docs/user/cli-reference.md) | Every `tumr` command |
| [Uninstall and your data](docs/user/uninstall-and-data.md) | Where everything is kept and how to remove it |

## Privacy and security

The runtime has no account and no telemetry, and it goes online only to download the tools you approved and to check for updates. Toolchains live in their own folder; your `PATH`, `JAVA_HOME`, `ANDROID_HOME` and Android Studio stay untouched. Agents get their own limited permissions, ask you before running a new folder, and every call is recorded.

Found a vulnerability? Please report it privately as described in [SECURITY.md](SECURITY.md).

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains how to build the project, run the checks and propose a change.

## Licence

Toolchain Universal Mobile Runtime is licensed under the [Apache License, Version 2.0](LICENSE); see also [NOTICE](NOTICE). If you use the project or build on it, a mention with a link back to this repository would be greatly appreciated. That is a request, not a condition of the licence. The name and logo follow the [trademark policy](TRADEMARKS.md).

Third-party components and their licences are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Android SDK components, the emulator, the JDK, Flutter and Node.js are downloaded from their publishers at your request and are never redistributed.

Android is a trademark of Google LLC. This project is not affiliated with or endorsed by Google.
