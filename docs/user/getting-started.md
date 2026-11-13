# Getting started

From nothing to your app in the preview with an agent fixing a bug — about 15 minutes plus download time.

## 1. Install
1. Download `AI-Mobile-Runtime_<version>_x64-setup.exe` (or the portable ZIP).
2. Check the file: `Get-FileHash .\AI-Mobile-Runtime_<version>_x64-setup.exe` must print the SHA-256 published with the release.
3. Run it. It installs for your Windows user only (no administrator rights) into `%LOCALAPPDATA%\AI Mobile Runtime` and adds a Start menu entry.

> Until releases are code-signed, Windows SmartScreen shows "Windows protected your PC". Choose **More info → Run anyway** only if the checksum matched.

The installer contains three programs: the desktop app, `umr` (the command line tool) and `umr-core` (the Runtime Core, which runs in the background and owns devices and sessions). To use `umr` from any terminal, add the install folder to your user `PATH`, or call it by its full path.

## 2. First setup (the wizard)
Start **AI Mobile Runtime** from the Start menu. The wizard:
1. **Scans** your PC (Windows build, virtualization, RAM, disk, existing JDKs, SDKs, Node, Flutter, Git). Nothing is changed.
2. Asks for a **project folder** and detects the framework (Kotlin/Java, Flutter, React Native, Expo).
3. Shows the **plan**: only what that project still needs (JDK, Android SDK packages, emulator image, Flutter, Node …), with sizes and where it goes. Existing installs are reused, read-only.
4. Shows each third-party **licence** (Android SDK, JDK …) for you to accept.
5. **Installs**, resumable, verifying every download. If Windows Hypervisor Platform is off, it asks once for administrator approval and a restart, then continues where it stopped.

The same works from a terminal: `umr doctor`, `umr plan <folder>`, `umr install <folder>`.

## 3. Your first app
In a terminal, inside your project folder:

```powershell
umr trust .        # once per folder: allows its build scripts to run
umr run            # builds, boots a phone, installs, launches, opens the preview
```

The preview docks on the right of your screen. Click and type in it like on a phone; the toolbar has back, home, recents, rotate, screenshot, reload, and **☰ / ⚠** for problems and logs.
Saved files reload by themselves (Flutter hot reload, Metro Fast Refresh). For Kotlin/Java use `umr reload full`.
Other phone sizes: `umr run --profile large` (or `compact`, `standard`, `foldable`, `flip`, `tablet`), or the device menu in the preview.

## 4. Connect your agent
```powershell
umr agent setup claude-code     # or: umr agent setup codex
```
It shows exactly what it will write (the MCP server entry and short instructions for the agent) and asks first. Start your agent in the project folder; for Claude Code, approve the `umr` MCP server when asked (`/mcp` lists it).

Then ask your agent something like: *"The app is running in the AI Mobile Runtime preview. Tapping the + button does nothing — find the problem with the umr tools, fix it and check the screen."* The agent will read the runtime status, open the problem, edit the code, reload and look at a screenshot. See [Agents](agents.md).

## 5. When something goes wrong
`umr problems` lists what failed, classified and with file and line; `umr logs -f` follows the app's log. The [troubleshooting guide](troubleshooting.md) has the common causes.

## Updates
`umr update --check` tells you whether a newer version exists; `umr update` downloads it, checks its checksum and installs it. Updates are checked only when you ask, against the release channel in `%USERPROFILE%\.umr\config.toml` (`[update] manifest_url`).
