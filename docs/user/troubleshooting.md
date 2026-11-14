# Troubleshooting

Start with `umr problems` (or ☰ in the preview window). Every problem has a **category** and an **origin**: *your code* means fix the project; *environment* means the toolchain, the device or the network.

## Build

### Compile error in your code

*Category: Build*

| | |
|---|---|
| Symptoms | `umr problems` lists an error with a file and line from `kotlinc`, `javac`, AAPT, Dart or Babel. |
| Likely cause | The code or a resource file does not compile. |
| How to check | `umr problems show <fingerprint>` for the full message. |
| Fix | Fix the file. Flutter and React Native reload by themselves after you save; Kotlin and Java: `umr reload full`. An error under `build/`, `node_modules/` or the pub cache comes from generated or third-party code — check versions before editing those files. |


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
| Symptoms | The app disappears from the screen; `umr problems` shows a crash or an ANR. |
| Likely cause | An uncaught Java or Kotlin exception, a native crash, or the app not responding. |
| How to check | `umr problems show <fingerprint>` shows the stack; `umr logs --source logcat --level E` the app's last log lines. |
| Fix | Fix the frame in your own code named in the stack, then `umr reload full` or `umr run`. |


## Environment

### SDK package missing

*Category: Environment*

| | |
|---|---|
| Symptoms | `SDK location not found`, `Failed to find target …` or `Failed to find Platform SDK …`. |
| Likely cause | An Android SDK package the project needs is not installed. |
| How to check | `umr plan .` lists what is missing. |
| Fix | `umr install .` |


### Licence not accepted

*Category: Environment*

| | |
|---|---|
| Symptoms | `licences have not been accepted`. |
| Likely cause | An SDK package's licence was not accepted yet. |
| How to check | `umr plan .` lists the licences still open. |
| Fix | `umr install .` shows each licence for you to accept. |


### Wrong JDK for the Gradle version

*Category: Environment*

| | |
|---|---|
| Symptoms | `requires Java 17` or `Unsupported class file major version`. |
| Likely cause | The JDK does not match the project's Gradle or Android Gradle Plugin. |
| How to check | `umr plan .` shows the JDK the project needs. |
| Fix | `umr install .` installs that JDK next to any others; nothing global changes. |


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
| Fix | `umr admin enable-whpx`, then restart Windows. If Task Manager shows virtualization disabled, turn it on in the firmware settings. |


### Port already in use

*Category: Environment*

| | |
|---|---|
| Symptoms | `address already in use` or port 8081 busy. |
| Likely cause | Another Metro or development server is running. |
| How to check | `umr logs --source tool --grep EADDRINUSE`. |
| Fix | Stop the other server, then `umr run` again. |


### Path too long

*Category: Environment*

| | |
|---|---|
| Symptoms | `filename … too long`. |
| Likely cause | The Windows path limit of 260 characters. |
| How to check | Count the project path plus the deepest `node_modules` or `build` folder. |
| Fix | `umr admin enable-long-paths`, or move the project to a shorter path. |


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
| How to check | `umr session` shows the device and its state. |
| Fix | `umr device use <profile> --cold` reboots it cleanly; `umr app uninstall <package>` removes an older copy with another signature. |


## Agent connection

Start with `umr agent doctor`: it checks every connected agent's config entry, the `umr.exe` it starts, its token and a real MCP handshake, and names the fix for each problem.

### The agent does not list the umr tools

*Category: Environment*

| | |
|---|---|
| Symptoms | `/mcp` (Claude Code, Codex), Cursor's MCP settings or VS Code's server list show no `umr` server, or it is switched off. |
| Likely cause | The entry is missing, the agent has not been restarted since setup, or the project's server is waiting for your approval. |
| How to check | `umr agent doctor <agent>` shows whether the entry is there and whether its handshake works. |
| Fix | Run `umr agent setup <agent>`, then restart the agent. Claude Code asks once to approve a project's `.mcp.json`; approve it, or switch it on in `/mcp`. Cursor and VS Code may ask you to enable a new server. Claude Desktop must be quit completely (also from the tray) and started again. |


### The umr server starts and stops at once

*Category: Environment*

| | |
|---|---|
| Symptoms | The agent reports that the `umr` server failed or closed; `umr agent doctor` says *program not found*, *points at another umr.exe* or *no token for client*. |
| Likely cause | The runtime moved (another install folder or the portable ZIP), or the agent's client was removed. |
| How to check | `umr agent doctor <agent>` |
| Fix | `umr agent doctor <agent> --fix` points the entry at the current `umr.exe`. For a missing token run `umr agent setup <agent>` again. |


### Setup refuses to edit a config file

*Category: Environment*

| | |
|---|---|
| Symptoms | `… is not plain JSON (…); comments and trailing commas are not supported, edit it by hand` |
| Likely cause | The agent's config file has comments or a syntax error. VS Code allows comments in `mcp.json`; the runtime never rewrites such a file, so your comments are not lost. |
| How to check | Open the file named in the message. |
| Fix | Remove the comments or fix the error, or add the entry yourself: `umr agent setup <agent> --print` shows it. |


### The agent waits and then reports a time-out

*Category: Environment*

| | |
|---|---|
| Symptoms | A tool call to `run_project` or `switch_device` hangs for up to two minutes, then fails. |
| Likely cause | The runtime asks you before an agent runs a new project folder or switches the device, and nobody answered. |
| How to check | `umr approvals` lists waiting requests; the preview window shows a banner. |
| Fix | Answer in the preview window or with `umr approvals allow <id> [--always]`. `--always` (or `umr agent grant <agent> <folder>`) stops it asking for that folder. |


## Still stuck

`umr doctor` checks the machine. When reporting a problem, attach `umr doctor --json`, `umr problems --json` and the newest file in `%USERPROFILE%\.umr\logs`. Output from `umr` is already sanitised; check raw log files before you share them.
