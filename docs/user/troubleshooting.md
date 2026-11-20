# Troubleshooting

Start with `tumr problems` (or ☰ in the preview window). Every problem has a **category** and an **origin**: *your code* means fix the project; *environment* means the toolchain, the device or the network.

## Build

### Compile error in your code

*Category: Build*

| | |
|---|---|
| Symptoms | `tumr problems` lists an error with a file and line from `kotlinc`, `javac`, AAPT, Dart or Babel. |
| Likely cause | The code or a resource file does not compile. |
| How to check | `tumr problems show <fingerprint>` for the full message. |
| Fix | Fix the file. Every framework rebuilds or reloads by itself after you save (Kotlin and Java: an incremental Gradle rebuild); `tumr reload full` forces one. An error under `build/`, `node_modules/` or the pub cache comes from generated or third-party code — check versions before editing those files. |


## Runtime and crash

### The app reports an error but keeps running

*Category: Runtime*

| | |
|---|---|
| Symptoms | A Flutter `EXCEPTION CAUGHT BY …`, an `Unhandled Exception` or a JavaScript error. |
| Likely cause | An exception in the app's code that the framework caught. |
| How to check | The problem's location points to the widget or the first frame in your code. |
| Fix | Fix the code at that location; the next reload clears the build problems it replaces. |


### The app closes

*Category: Crash*

| | |
|---|---|
| Symptoms | The app disappears from the screen; `tumr problems` shows a crash or an ANR. |
| Likely cause | An uncaught Java or Kotlin exception, a native crash, or the app not responding. |
| How to check | `tumr problems show <fingerprint>` shows the stack; `tumr logs --source logcat --level E` the app's last log lines. |
| Fix | Fix the frame in your own code named in the stack and save (the app rebuilds and starts again), or run `tumr reload full` / `tumr run`. |


## Environment

### SDK package missing

*Category: Environment*

| | |
|---|---|
| Symptoms | `SDK location not found`, `Failed to find target …` or `Failed to find Platform SDK …`. |
| Likely cause | An Android SDK package the project needs is not installed. |
| How to check | `tumr plan .` lists what is missing. |
| Fix | `tumr install .` |


### Licence not accepted

*Category: Environment*

| | |
|---|---|
| Symptoms | `licences have not been accepted`. |
| Likely cause | An SDK package's licence was not accepted yet. |
| How to check | `tumr plan .` lists the licences still open. |
| Fix | `tumr install .` shows each licence for you to accept. |


### Wrong JDK for the Gradle version

*Category: Environment*

| | |
|---|---|
| Symptoms | `requires Java 17` or `Unsupported class file major version`. |
| Likely cause | The JDK does not match the project's Gradle or Android Gradle Plugin. |
| How to check | `tumr plan .` shows the JDK the project needs. |
| Fix | `tumr install .` installs that JDK next to any others; nothing global changes. |


### Gradle runs out of memory

*Category: Environment*

| | |
|---|---|
| Symptoms | `Java heap space` or `Gradle build daemon disappeared unexpectedly`. |
| Likely cause | Gradle's memory limit is too low for the project. |
| How to check | Task Manager shows memory nearly full while Gradle builds. |
| Fix | Close other heavy apps, or add `org.gradle.jvmargs=-Xmx4g` to the project's `gradle.properties`. |


### The emulator has no acceleration

*Category: Environment*

| | |
|---|---|
| Symptoms | `hardware acceleration` or `WHPX` errors when the device boots. |
| Likely cause | CPU virtualization is off in the firmware, or Windows Hypervisor Platform is off. |
| How to check | See the layers on the [requirements page](/docs/requirements/#how-virtualization-fits-together). |
| Fix | `tumr admin enable-whpx`, then restart Windows. If Task Manager shows virtualization disabled, turn it on in the firmware settings. |


### Port already in use

*Category: Environment*

| | |
|---|---|
| Symptoms | `address already in use` or port 8081 busy. |
| Likely cause | Another Metro or development server is running. |
| How to check | `tumr logs --source tool --grep EADDRINUSE`. |
| Fix | Stop the other server, then `tumr run` again. |


### Path too long

*Category: Environment*

| | |
|---|---|
| Symptoms | `filename … too long`, or `ninja: error: Stat(…): Filename longer than 260 characters` from `buildCMakeDebug`. |
| Likely cause | The Windows path limit of 260 characters. React Native native modules (for example `react-native-safe-area-context`) compile C++ files into very deep `android/app/.cxx` paths, and ninja ignores the long-paths setting. |
| How to check | Count the project path plus the deepest `node_modules`, `build` or `.cxx` folder. |
| Fix | Move the project to a shorter path (such as `C:\dev\app`), then delete `android/app/.cxx` and run again. `tumr admin enable-long-paths` helps Gradle and Node, not ninja. |


## Network

### A download or the dev server cannot be reached

*Category: Network*

| | |
|---|---|
| Symptoms | `Could not GET …`, `UnknownHostException`, `PKIX path building failed`, `ENOTFOUND` or `Could not connect to development server`. |
| Likely cause | No connection, a proxy, or a company proxy inspecting TLS. |
| How to check | Open the failing address in a browser on the same PC. |
| Fix | Fix the connection or proxy. For TLS-inspecting company proxies, their root certificate must be in the Windows certificate store; the runtime uses that store. |


## Device

### Device offline or app will not install

*Category: Device*

| | |
|---|---|
| Symptoms | `device offline`, `unauthorized`, `INSTALL_FAILED_INSUFFICIENT_STORAGE` or `INSTALL_FAILED_UPDATE_INCOMPATIBLE`. |
| Likely cause | The emulator stopped answering, is full, or holds an older copy signed with another key. |
| How to check | `tumr session` shows the device and its state. |
| Fix | `tumr device use <profile> --cold` reboots it cleanly; `tumr app uninstall <package>` removes an older copy with another signature. |


## Agent connection

Start with `tumr agent doctor`: it checks every connected agent's config entry, the `tumr.exe` it starts, its token and a real MCP handshake, and names the fix for each problem.

### The agent does not list the tumr tools

*Category: Environment*

| | |
|---|---|
| Symptoms | `/mcp` (Claude Code, Codex), Cursor's MCP settings, Antigravity's MCP Servers or VS Code's server list show no `tumr` server, or it is switched off. |
| Likely cause | The entry is missing, the agent has not been restarted since setup, or the project's server is waiting for your approval. |
| How to check | `tumr agent doctor <agent>` shows whether the entry is there and whether its handshake works. |
| Fix | Run `tumr agent setup <agent>`, then restart the agent. Claude Code asks once to approve a project's `.mcp.json`; approve it, or switch it on in `/mcp`. Cursor and VS Code may ask you to enable a new server. Claude Desktop must be quit completely (also from the tray) and started again. |


### The tumr server starts and stops at once

*Category: Environment*

| | |
|---|---|
| Symptoms | The agent reports that the `tumr` server failed or closed; `tumr agent doctor` says *program not found*, *points at another tumr.exe* or *no token for client*. |
| Likely cause | The runtime moved (another install folder or the portable ZIP), or the agent's client was removed. |
| How to check | `tumr agent doctor <agent>` |
| Fix | `tumr agent doctor <agent> --fix` points the entry at the current `tumr.exe`. For a missing token run `tumr agent setup <agent>` again. |


### Setup refuses to edit a config file

*Category: Environment*

| | |
|---|---|
| Symptoms | `… is not plain JSON (…); comments and trailing commas are not supported, edit it by hand` |
| Likely cause | The agent's config file has comments or a syntax error. VS Code allows comments in `mcp.json`; the runtime never rewrites such a file, so your comments are not lost. |
| How to check | Open the file named in the message. |
| Fix | Remove the comments or fix the error, or add the entry yourself: `tumr agent setup <agent> --print` shows it. |


### The agent waits and then reports a time-out

*Category: Environment*

| | |
|---|---|
| Symptoms | A tool call to `run_project` or `switch_device` hangs for up to two minutes, then fails. |
| Likely cause | The runtime asks you before an agent runs a new project folder or switches the device, and nobody answered. |
| How to check | `tumr approvals` lists waiting requests; the preview window shows a banner. |
| Fix | Answer in the preview window or with `tumr approvals allow <id> [--always]`. `--always` (or `tumr agent grant <agent> <folder>`) stops it asking for that folder. |


## Still stuck

`tumr doctor` checks the machine. When reporting a problem, attach `tumr doctor --json`, `tumr problems --json` and the newest file in `%USERPROFILE%\.tumr\logs`. Output from `tumr` is already sanitised; check raw log files before you share them.
