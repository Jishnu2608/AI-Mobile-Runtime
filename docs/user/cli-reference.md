# `tumr` command reference

Generated from `tumr --help` by `scripts/cli-reference.ps1` - do not edit by hand.

```text
Toolchain Universal Mobile Runtime — run and preview mobile apps next to your coding agent

Usage: tumr <COMMAND>

Commands:
  status      Show whether the Runtime Core is running
  start       Start the Runtime Core in the background (no-op if already running)
  stop        Stop the Runtime Core
  doctor      Check this machine (starts the Runtime Core if needed)
  plan        Show what a project needs and what would be installed (installs nothing, changes nothing)
  install     Install what `tumr plan` lists into the managed folder (asks before anything is installed)
  components  List installed components, check that they work, and show disk usage
  remove      Remove one component the product installed (id from `tumr components`)
  repair      Remove broken components; with a project folder, install them again
  project     Show what kind of project a folder is, with the evidence, and whether it is trusted
  trust       Trust a project folder so its build scripts may run (or `--revoke` the trust)
  device      Devices seen by the product's adb server
  run         Build, install and launch a project on the session's device (Kotlin/Java via Gradle, Flutter, React Native / Expo) and open the preview. The folder must be trusted and its toolchain installed
  preview     The live preview window (docked beside your terminal), its status and the latency self-test
  problems    Problems of the current run, classified (build, runtime, crash, environment, device, network) and deduplicated; `tumr problems show <fingerprint>` for one with its stack
  logs        Log lines of the current run: tool output, progress and the app's device log
  mcp         MCP server for an AI coding agent on stdin/stdout (the agent starts it; see `tumr agent setup`)
  agent       Connect AI coding agents: setup, clients, grants, audit log
  approvals   Requests from agents waiting for your answer (running a new project folder, switching the device)
  reload      Apply code changes to the running app: hot (default), restart or full
  adapter     Framework adapters
  session     The current runtime session (device and app)
  app         Install, launch, stop or uninstall an app on the current device
  screenshot  Save a screenshot of the current device (PNG)
  ui          Show the UI tree of the current screen (text, ids, bounds, clickable elements)
  input       Send input to the current device
  update      Check for a newer version and install it (installed copies; portable copies get a link)
  clean       Delete the download cache and old logs
  path        The install folder on your user PATH, so `tumr` works in any new terminal (off by default): `status`, `add`, `remove`. `restore` (the installer) adds it back only if you chose it
  admin       Run one administrator action through a Windows UAC prompt: `enable-whpx` or `enable-long-paths`
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## tumr status

```text
Show whether the Runtime Core is running

Usage: tumr status [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## tumr start

```text
Start the Runtime Core in the background (no-op if already running)

Usage: tumr start

Options:
  -h, --help  Print help
```

## tumr stop

```text
Stop the Runtime Core

Usage: tumr stop

Options:
  -h, --help  Print help
```

## tumr doctor

```text
Check this machine (starts the Runtime Core if needed)

Usage: tumr doctor [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## tumr plan

```text
Show what a project needs and what would be installed (installs nothing, changes nothing)

Usage: tumr plan [OPTIONS] [PATH]

Arguments:
  [PATH]  Project folder (default: current folder)

Options:
      --rescan  Scan the environment again instead of using the last scan
      --json    Print machine-readable JSON
  -h, --help    Print help
```

## tumr install

```text
Install what `tumr plan` lists into the managed folder (asks before anything is installed)

Usage: tumr install [OPTIONS] [PATH]

Arguments:
  [PATH]  Project folder (default: current folder)

Options:
      --yes   Skip the final confirmation (licences are still shown and must be accepted)
  -h, --help  Print help
```

## tumr components

```text
List installed components, check that they work, and show disk usage

Usage: tumr components [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## tumr remove

```text
Remove one component the product installed (id from `tumr components`)

Usage: tumr remove [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --force  Remove even if other components need it
      --yes    Do not ask for confirmation
  -h, --help   Print help
```

## tumr repair

```text
Remove broken components; with a project folder, install them again

Usage: tumr repair [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --yes   Skip the final install confirmation
  -h, --help  Print help
```

## tumr project

```text
Show what kind of project a folder is, with the evidence, and whether it is trusted

Usage: tumr project [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## tumr trust

```text
Trust a project folder so its build scripts may run (or `--revoke` the trust)

Usage: tumr trust [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --revoke  
      --yes     Do not ask for confirmation
  -h, --help    Print help
```

## tumr device

```text
Devices seen by the product's adb server

Usage: tumr device <COMMAND>

Commands:
  list         List devices and emulators (starts the product's private adb server if needed)
  profiles     Show the device size classes (compact, standard, large, fold, flip, tablet)
  create       Create the emulator (AVD) for a size class from the product's own hardware definition
  boot         Boot the emulator for a size class headless (creates it if needed). Warm boots from the product's snapshot when the GPU mode still matches; checks that the screen renders
  stop         Stop an emulator the product started
  use          Start a session on a size class: boot it (switching from another device), optionally install and launch an app
  rotate       Rotate the emulator 90° (left or right)
  posture      Fold or unfold a foldable emulator while it runs: a posture (closed, half-opened, opened) or `--angle` in degrees (0 closed, 90 half open, 180 open flat). Closed, the app moves to the cover screen. Without either it shows the hinge as it is
  fingerprint  Touch the emulator's fingerprint sensor: unlocks the lock screen and answers an app's biometric prompt once a finger is enrolled (Settings, with a screen lock set). `down` / `up` hold the finger, e.g. while Settings enrols it
  auth         Lock screen and fingerprint state: locked, sensor waiting for a finger, fingers enrolled
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr device list

```text
List devices and emulators (starts the product's private adb server if needed)

Usage: tumr device list [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

### tumr device profiles

```text
Show the device size classes (compact, standard, large, fold, flip, tablet)

Usage: tumr device profiles [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

### tumr device create

```text
Create the emulator (AVD) for a size class from the product's own hardware definition

Usage: tumr device create [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>  API level of the system image (default: newest installed)
  -h, --help       Print help
```

### tumr device boot

```text
Boot the emulator for a size class headless (creates it if needed). Warm boots from the product's snapshot when the GPU mode still matches; checks that the screen renders

Usage: tumr device boot [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>  
      --cold       Ignore the warm snapshot and boot cold
      --window     Show the emulator's own window (debugging)
      --gpu <GPU>  GPU mode: `host` (default), or software `swiftshader_indirect` / `angle_indirect`
      --json       Print machine-readable JSON
  -h, --help       Print help
```

### tumr device stop

```text
Stop an emulator the product started

Usage: tumr device stop [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>  
  -h, --help       Print help
```

### tumr device use

```text
Start a session on a size class: boot it (switching from another device), optionally install and launch an app

Usage: tumr device use [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>          
      --cold               
      --apk <APK>          APK to install and launch after boot
      --package <PACKAGE>  Package to launch (already installed, or the APK's package)
      --json               
  -h, --help               Print help
```

### tumr device rotate

```text
Rotate the emulator 90° (left or right)

Usage: tumr device rotate [DIRECTION]

Arguments:
  [DIRECTION]  [default: left]

Options:
  -h, --help  Print help
```

### tumr device posture

```text
Fold or unfold a foldable emulator while it runs: a posture (closed, half-opened, opened) or `--angle` in degrees (0 closed, 90 half open, 180 open flat). Closed, the app moves to the cover screen. Without either it shows the hinge as it is

Usage: tumr device posture [OPTIONS] [POSTURE]

Arguments:
  [POSTURE]  

Options:
      --angle <ANGLE>    Hinge angle in degrees, 0–180
      --target <TARGET>  
  -h, --help             Print help
```

### tumr device fingerprint

```text
Touch the emulator's fingerprint sensor: unlocks the lock screen and answers an app's biometric prompt once a finger is enrolled (Settings, with a screen lock set). `down` / `up` hold the finger, e.g. while Settings enrols it

Usage: tumr device fingerprint [OPTIONS] [TOUCH]

Arguments:
  [TOUCH]  tap (default), down or up [default: tap]

Options:
      --finger <FINGER>  Enrolled finger, 1-10 [default: 1]
      --target <TARGET>  
  -h, --help             Print help
```

### tumr device auth

```text
Lock screen and fingerprint state: locked, sensor waiting for a finger, fingers enrolled

Usage: tumr device auth [OPTIONS]

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## tumr run

```text
Build, install and launch a project on the session's device (Kotlin/Java via Gradle, Flutter, React Native / Expo) and open the preview. The folder must be trusted and its toolchain installed

Usage: tumr run [OPTIONS] [PATH]

Arguments:
  [PATH]  Project folder (default: current folder)

Options:
      --profile <PROFILE>  Device size class (default: standard)
      --cold               Boot the device cold
      --stop               Stop the running app and its tools instead
      --no-watch           Do not reload on saved files
      --no-preview         Do not open the preview window
      --json               
  -h, --help               Print help
```

## tumr preview

```text
The live preview window (docked beside your terminal), its status and the latency self-test

Usage: tumr preview [OPTIONS] [COMMAND]

Commands:
  status    Frames, windows and the time the runtime adds
  selftest  Touch-to-screen latency and frame rate on the device; taps the home screen
  help      Print this message or the help of the given subcommand(s)

Options:
      --dock <DOCK>  Where the window sits: right (default), left or float [default: right]
  -h, --help         Print help
```

### tumr preview status

```text
Frames, windows and the time the runtime adds

Usage: tumr preview status [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### tumr preview selftest

```text
Touch-to-screen latency and frame rate on the device; taps the home screen

Usage: tumr preview selftest [OPTIONS]

Options:
      --taps <TAPS>  [default: 20]
      --json         
  -h, --help         Print help
```

## tumr problems

```text
Problems of the current run, classified (build, runtime, crash, environment, device, network) and deduplicated; `tumr problems show <fingerprint>` for one with its stack

Usage: tumr problems [OPTIONS] [COMMAND]

Commands:
  show  One problem with its stack
  help  Print this message or the help of the given subcommand(s)

Options:
      --category <CATEGORY>  Only this category
      --json                 
  -h, --help                 Print help
```

### tumr problems show

```text
One problem with its stack

Usage: tumr problems show [OPTIONS] <FINGERPRINT>

Arguments:
  <FINGERPRINT>  

Options:
      --json  
  -h, --help  Print help
```

## tumr logs

```text
Log lines of the current run: tool output, progress and the app's device log

Usage: tumr logs [OPTIONS]

Options:
      --source <SOURCE>  `tool`, `tumr` or `logcat`
      --level <LEVEL>    Lowest level: V, D, I, W, E or F
      --grep <GREP>      Only lines containing this text (or tag)
  -n, --lines <LINES>    How many recent lines [default: 100]
  -f, --follow           Keep printing new lines
      --json             One JSON object per line
  -h, --help             Print help
```

## tumr mcp

```text
MCP server for an AI coding agent on stdin/stdout (the agent starts it; see `tumr agent setup`)

Usage: tumr mcp [OPTIONS]

Options:
      --client <CLIENT>  The agent client whose token and permissions to use [default: claude-code]
      --list-tools       Print the tool catalogue as JSON and exit (no Core needed; used for the docs)
  -h, --help             Print help
```

## tumr agent

```text
Connect AI coding agents: setup, clients, grants, audit log

Usage: tumr agent <COMMAND>

Commands:
  setup         Connect a coding agent (claude-code, codex, cursor, claude-desktop, vscode, antigravity): creates its client, writes its MCP server entry and agent instructions (shows the changes and asks first). `--remove` disconnects it again
  hook          The agent's stop hook (written by `setup` for claude-code, codex, cursor and antigravity): reads the hook input on stdin, waits for the rebuild the agent's saves started, and keeps the agent working while the app has a build error, a new runtime error or a crash. Never blocks when nothing of this folder runs
  link          One-click install link for Cursor or VS Code, made for this PC's tumr (creates the agent client if needed)
  repoint       DEC-055: point every all-projects agent entry that runs another tumr at this one (the installer runs it after an install). Changes nothing else and starts nothing
  doctor        Check each connected agent end to end: config entry, program path, token, a real MCP handshake and its last call. `--fix` repoints entries at this tumr
  add           Create an agent client with its own token (default scopes: observe, interact, control)
  clients       Coding agents on this PC (Claude Code, Codex, Cursor, Claude Desktop, VS Code, Antigravity): installed, connected, and how to connect each one. Does not need the Core
  list          Agent clients, their scopes and grants
  remove        Remove an agent client (its token stops working)
  grant         Let an agent run a project folder (or `device:switch`) without asking each time
  ungrant       Take a grant back
  audit         What agents called (newest last)
  instructions  Print the agent instructions (`skill` = Claude Code SKILL.md, `agents` = AGENTS.md section, `short` = MCP instructions)
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr agent setup

```text
Connect a coding agent (claude-code, codex, cursor, claude-desktop, vscode, antigravity): creates its client, writes its MCP server entry and agent instructions (shows the changes and asks first). `--remove` disconnects it again

Usage: tumr agent setup [OPTIONS] <AGENT>

Arguments:
  <AGENT>  claude-code, codex, cursor, claude-desktop, vscode or antigravity

Options:
      --project <PROJECT>  Project folder (project-scope config and instruction files); default: current folder
      --scope <SCOPE>      `project` (this folder) or `user` (all projects). Default: project for Claude Code, Cursor and VS Code; user for Codex, Claude Desktop and Antigravity
      --remove             Take the runtime out of the agent's files and revoke its token
      --print              Only print the server entry to add by hand; write nothing
      --yes                Do not ask
      --dry-run            Only show what would change
      --json               Machine-readable plan / result; never asks (applies only with --yes)
  -h, --help               Print help
```

### tumr agent hook

```text
The agent's stop hook (written by `setup` for claude-code, codex, cursor and antigravity): reads the hook input on stdin, waits for the rebuild the agent's saves started, and keeps the agent working while the app has a build error, a new runtime error or a crash. Never blocks when nothing of this folder runs

Usage: tumr agent hook [OPTIONS] <AGENT>

Arguments:
  <AGENT>  claude-code, codex, cursor or antigravity

Options:
      --after-edit  AUD-P1-32: run after a file edit inside the agent's turn (Claude Code `PostToolUse`): report problems the app already shows, without waiting for a build
  -h, --help        Print help
```

### tumr agent link

```text
One-click install link for Cursor or VS Code, made for this PC's tumr (creates the agent client if needed)

Usage: tumr agent link [OPTIONS] <AGENT>

Arguments:
  <AGENT>  cursor or vscode

Options:
      --json  
  -h, --help  Print help
```

### tumr agent repoint

```text
DEC-055: point every all-projects agent entry that runs another tumr at this one (the installer runs it after an install). Changes nothing else and starts nothing

Usage: tumr agent repoint

Options:
  -h, --help  Print help
```

### tumr agent doctor

```text
Check each connected agent end to end: config entry, program path, token, a real MCP handshake and its last call. `--fix` repoints entries at this tumr

Usage: tumr agent doctor [OPTIONS] [AGENT]

Arguments:
  [AGENT]  Only this agent

Options:
      --project <PROJECT>  Project folder whose project-scope configs are checked; default: current folder
      --fix                Rewrite entries whose program path is missing or points at another tumr
      --json               
  -h, --help               Print help
```

### tumr agent add

```text
Create an agent client with its own token (default scopes: observe, interact, control)

Usage: tumr agent add [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --scopes <SCOPES>  Comma-separated: observe, interact, control
  -h, --help             Print help
```

### tumr agent clients

```text
Coding agents on this PC (Claude Code, Codex, Cursor, Claude Desktop, VS Code, Antigravity): installed, connected, and how to connect each one. Does not need the Core

Usage: tumr agent clients [OPTIONS]

Options:
      --project <PROJECT>  Project folder whose config files are checked; default: current folder
      --json               
  -h, --help               Print help
```

### tumr agent list

```text
Agent clients, their scopes and grants

Usage: tumr agent list [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### tumr agent remove

```text
Remove an agent client (its token stops working)

Usage: tumr agent remove <NAME>

Arguments:
  <NAME>  

Options:
  -h, --help  Print help
```

### tumr agent grant

```text
Let an agent run a project folder (or `device:switch`) without asking each time

Usage: tumr agent grant <NAME> <WHAT>

Arguments:
  <NAME>  
  <WHAT>  

Options:
  -h, --help  Print help
```

### tumr agent ungrant

```text
Take a grant back

Usage: tumr agent ungrant <NAME> <WHAT>

Arguments:
  <NAME>  
  <WHAT>  

Options:
  -h, --help  Print help
```

### tumr agent audit

```text
What agents called (newest last)

Usage: tumr agent audit [OPTIONS]

Options:
      --client <CLIENT>  Only this agent
  -n, --lines <LINES>    [default: 30]
      --json             
  -h, --help             Print help
```

### tumr agent instructions

```text
Print the agent instructions (`skill` = Claude Code SKILL.md, `agents` = AGENTS.md section, `short` = MCP instructions)

Usage: tumr agent instructions [OPTIONS]

Options:
      --format <FORMAT>  [default: skill]
  -h, --help             Print help
```

## tumr approvals

```text
Requests from agents waiting for your answer (running a new project folder, switching the device)

Usage: tumr approvals [COMMAND]

Commands:
  allow  Allow a request
  deny   Deny a request
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr approvals allow

```text
Allow a request

Usage: tumr approvals allow [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --always  Remember it for this agent (records a grant)
  -h, --help    Print help
```

### tumr approvals deny

```text
Deny a request

Usage: tumr approvals deny <ID>

Arguments:
  <ID>  

Options:
  -h, --help  Print help
```

## tumr reload

```text
Apply code changes to the running app: hot (default), restart or full

Usage: tumr reload [OPTIONS] [KIND]

Arguments:
  [KIND]  

Options:
      --json  
  -h, --help  Print help
```

## tumr adapter

```text
Framework adapters

Usage: tumr adapter <COMMAND>

Commands:
  check  Run the adapter contract test kit live: start, every reload kind, stop
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr adapter check

```text
Run the adapter contract test kit live: start, every reload kind, stop

Usage: tumr adapter check [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --profile <PROFILE>  
      --json               
  -h, --help               Print help
```

## tumr session

```text
The current runtime session (device and app)

Usage: tumr session [COMMAND]

Commands:
  status  State, recent events, supervised processes and ports (default)
  stop    Stop the session and its emulator
  follow  Print each session change as it happens (state, device, app) until Ctrl+C. Uses `session.wait`, so nothing polls
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr session status

```text
State, recent events, supervised processes and ports (default)

Usage: tumr session status [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### tumr session stop

```text
Stop the session and its emulator

Usage: tumr session stop

Options:
  -h, --help  Print help
```

### tumr session follow

```text
Print each session change as it happens (state, device, app) until Ctrl+C. Uses `session.wait`, so nothing polls

Usage: tumr session follow [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

## tumr app

```text
Install, launch, stop or uninstall an app on the current device

Usage: tumr app <COMMAND>

Commands:
  install    
  launch     
  stop       
  uninstall  
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### tumr app install

```text
Usage: tumr app install [OPTIONS] <APK>

Arguments:
  <APK>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### tumr app launch

```text
Usage: tumr app launch [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --activity <ACTIVITY>  Activity (`.Main` or `pkg/.Main`); default: the launcher activity
      --target <TARGET>      
  -h, --help                 Print help
```

### tumr app stop

```text
Usage: tumr app stop [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### tumr app uninstall

```text
Usage: tumr app uninstall [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## tumr screenshot

```text
Save a screenshot of the current device (PNG)

Usage: tumr screenshot [OPTIONS]

Options:
      --out <OUT>        Copy the PNG here as well
      --target <TARGET>  Device serial (default: the current session's device)
      --json             
  -h, --help             Print help
```

## tumr ui

```text
Show the UI tree of the current screen (text, ids, bounds, clickable elements)

Usage: tumr ui [OPTIONS]

Options:
      --target <TARGET>  
      --json             
  -h, --help             Print help
```

## tumr input

```text
Send input to the current device

Usage: tumr input [OPTIONS] <COMMAND>

Commands:
  tap    Tap at device pixels
  swipe  Swipe from (x1, y1) to (x2, y2)
  text   Type text into the focused field
  key    Press a key: home, back, recents, enter, delete, tab, escape, menu, power, `volume_up`, `volume_down`, up, down, left, right
  help   Print this message or the help of the given subcommand(s)

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### tumr input tap

```text
Tap at device pixels

Usage: tumr input tap [OPTIONS] <X> <Y>

Arguments:
  <X>  
  <Y>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### tumr input swipe

```text
Swipe from (x1, y1) to (x2, y2)

Usage: tumr input swipe [OPTIONS] <X1> <Y1> <X2> <Y2>

Arguments:
  <X1>  
  <Y1>  
  <X2>  
  <Y2>  

Options:
      --ms <MS>          [default: 300]
      --target <TARGET>  
  -h, --help             Print help
```

### tumr input text

```text
Type text into the focused field

Usage: tumr input text [OPTIONS] <TEXT>

Arguments:
  <TEXT>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### tumr input key

```text
Press a key: home, back, recents, enter, delete, tab, escape, menu, power, `volume_up`, `volume_down`, up, down, left, right

Usage: tumr input key [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## tumr update

```text
Check for a newer version and install it (installed copies; portable copies get a link)

Usage: tumr update [OPTIONS]

Options:
      --check     Only check, do not install
      --yes       Do not ask
      --relaunch  Open the desktop app again once the new version is installed (the app's Update now button uses this)
      --json      
  -h, --help      Print help
```

## tumr clean

```text
Delete the download cache and old logs

Usage: tumr clean

Options:
  -h, --help  Print help
```

## tumr path

```text
The install folder on your user PATH, so `tumr` works in any new terminal (off by default): `status`, `add`, `remove`. `restore` (the installer) adds it back only if you chose it

Usage: tumr path [OPTIONS] [ACTION]

Arguments:
  [ACTION]  [default: status]

Options:
      --keep-choice  With `remove`: keep the choice, so the next install adds it back (the uninstaller)
      --json         
  -h, --help         Print help
```

## tumr admin

```text
Run one administrator action through a Windows UAC prompt: `enable-whpx` or `enable-long-paths`

Usage: tumr admin [OPTIONS] <ACTION>

Arguments:
  <ACTION>  

Options:
      --yes   Do not ask for confirmation (Windows still shows its UAC prompt)
  -h, --help  Print help
```
