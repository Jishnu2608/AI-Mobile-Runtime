# `umr` command reference

Generated from `umr --help` by `scripts/cli-reference.ps1` - do not edit by hand.

```text
AI Mobile Runtime ΓÇö run and preview mobile apps next to your coding agent

Usage: umr <COMMAND>

Commands:
  status      Show whether the Runtime Core is running
  start       Start the Runtime Core in the background (no-op if already running)
  stop        Stop the Runtime Core
  doctor      Check this machine (starts the Runtime Core if needed)
  plan        Show what a project needs and what would be installed (installs nothing, changes nothing)
  install     Install what `umr plan` lists into the managed folder (asks before anything is installed)
  components  List installed components, check that they work, and show disk usage
  remove      Remove one component the product installed (id from `umr components`)
  repair      Remove broken components; with a project folder, install them again
  project     Show what kind of project a folder is, with the evidence, and whether it is trusted
  trust       Trust a project folder so its build scripts may run (or `--revoke` the trust)
  device      Devices seen by the product's adb server
  run         Build, install and launch a project on the session's device (Kotlin/Java via Gradle, Flutter, React Native / Expo) and open the preview. The folder must be trusted and its toolchain installed
  preview     The live preview window (docked beside your terminal), its status and the latency self-test
  problems    Problems of the current run, classified (build, runtime, crash, environment, device, network) and deduplicated; `umr problems show <fingerprint>` for one with its stack
  logs        Log lines of the current run: tool output, progress and the app's device log
  mcp         MCP server for an AI coding agent on stdin/stdout (the agent starts it; see `umr agent setup`)
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
  admin       Run one administrator action through a Windows UAC prompt: `enable-whpx` or `enable-long-paths`
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## umr status

```text
Show whether the Runtime Core is running

Usage: umr status [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## umr start

```text
Start the Runtime Core in the background (no-op if already running)

Usage: umr start

Options:
  -h, --help  Print help
```

## umr stop

```text
Stop the Runtime Core

Usage: umr stop

Options:
  -h, --help  Print help
```

## umr doctor

```text
Check this machine (starts the Runtime Core if needed)

Usage: umr doctor [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## umr plan

```text
Show what a project needs and what would be installed (installs nothing, changes nothing)

Usage: umr plan [OPTIONS] [PATH]

Arguments:
  [PATH]  Project folder (default: current folder)

Options:
      --rescan  Scan the environment again instead of using the last scan
      --json    Print machine-readable JSON
  -h, --help    Print help
```

## umr install

```text
Install what `umr plan` lists into the managed folder (asks before anything is installed)

Usage: umr install [OPTIONS] [PATH]

Arguments:
  [PATH]  Project folder (default: current folder)

Options:
      --yes   Skip the final confirmation (licences are still shown and must be accepted)
  -h, --help  Print help
```

## umr components

```text
List installed components, check that they work, and show disk usage

Usage: umr components [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## umr remove

```text
Remove one component the product installed (id from `umr components`)

Usage: umr remove [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --force  Remove even if other components need it
      --yes    Do not ask for confirmation
  -h, --help   Print help
```

## umr repair

```text
Remove broken components; with a project folder, install them again

Usage: umr repair [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --yes   Skip the final install confirmation
  -h, --help  Print help
```

## umr project

```text
Show what kind of project a folder is, with the evidence, and whether it is trusted

Usage: umr project [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

## umr trust

```text
Trust a project folder so its build scripts may run (or `--revoke` the trust)

Usage: umr trust [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --revoke  
      --yes     Do not ask for confirmation
  -h, --help    Print help
```

## umr device

```text
Devices seen by the product's adb server

Usage: umr device <COMMAND>

Commands:
  list      List devices and emulators (starts the product's private adb server if needed)
  profiles  Show the device size classes (compact, standard, large, fold, flip, tablet)
  create    Create the emulator (AVD) for a size class from the product's own hardware definition
  boot      Boot the emulator for a size class headless (creates it if needed). Warm boots from the product's snapshot when the GPU mode still matches; checks that the screen renders
  stop      Stop an emulator the product started
  use       Start a session on a size class: boot it (switching from another device), optionally install and launch an app
  rotate    Rotate the emulator 90┬░ (left or right)
  posture   Fold or unfold a foldable emulator while it runs: closed, half-opened, opened, flipped, tent
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### umr device list

```text
List devices and emulators (starts the product's private adb server if needed)

Usage: umr device list [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

### umr device profiles

```text
Show the device size classes (compact, standard, large, fold, flip, tablet)

Usage: umr device profiles [OPTIONS]

Options:
      --json  Print machine-readable JSON
  -h, --help  Print help
```

### umr device create

```text
Create the emulator (AVD) for a size class from the product's own hardware definition

Usage: umr device create [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>  API level of the system image (default: newest installed)
  -h, --help       Print help
```

### umr device boot

```text
Boot the emulator for a size class headless (creates it if needed). Warm boots from the product's snapshot when the GPU mode still matches; checks that the screen renders

Usage: umr device boot [OPTIONS] <PROFILE>

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

### umr device stop

```text
Stop an emulator the product started

Usage: umr device stop [OPTIONS] <PROFILE>

Arguments:
  <PROFILE>  

Options:
      --api <API>  
  -h, --help       Print help
```

### umr device use

```text
Start a session on a size class: boot it (switching from another device), optionally install and launch an app

Usage: umr device use [OPTIONS] <PROFILE>

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

### umr device rotate

```text
Rotate the emulator 90┬░ (left or right)

Usage: umr device rotate [DIRECTION]

Arguments:
  [DIRECTION]  [default: left]

Options:
  -h, --help  Print help
```

### umr device posture

```text
Fold or unfold a foldable emulator while it runs: closed, half-opened, opened, flipped, tent

Usage: umr device posture [OPTIONS] <POSTURE>

Arguments:
  <POSTURE>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## umr run

```text
Build, install and launch a project on the session's device (Kotlin/Java via Gradle, Flutter, React Native / Expo) and open the preview. The folder must be trusted and its toolchain installed

Usage: umr run [OPTIONS] [PATH]

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

## umr preview

```text
The live preview window (docked beside your terminal), its status and the latency self-test

Usage: umr preview [OPTIONS] [COMMAND]

Commands:
  status    Frames, windows and the time the product adds (NFR-02a)
  selftest  Touch ΓåÆ visible latency and frame rate on the device (NFR-02b / NFR-03); taps the home screen
  help      Print this message or the help of the given subcommand(s)

Options:
      --dock <DOCK>  Where the window sits: right (default), left or float [default: right]
  -h, --help         Print help
```

### umr preview status

```text
Frames, windows and the time the product adds (NFR-02a)

Usage: umr preview status [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### umr preview selftest

```text
Touch ΓåÆ visible latency and frame rate on the device (NFR-02b / NFR-03); taps the home screen

Usage: umr preview selftest [OPTIONS]

Options:
      --taps <TAPS>  [default: 20]
      --json         
  -h, --help         Print help
```

## umr problems

```text
Problems of the current run, classified (build, runtime, crash, environment, device, network) and deduplicated; `umr problems show <fingerprint>` for one with its stack

Usage: umr problems [OPTIONS] [COMMAND]

Commands:
  show  One problem with its stack
  help  Print this message or the help of the given subcommand(s)

Options:
      --category <CATEGORY>  Only this category
      --json                 
  -h, --help                 Print help
```

### umr problems show

```text
One problem with its stack

Usage: umr problems show [OPTIONS] <FINGERPRINT>

Arguments:
  <FINGERPRINT>  

Options:
      --json  
  -h, --help  Print help
```

## umr logs

```text
Log lines of the current run: tool output, progress and the app's device log

Usage: umr logs [OPTIONS]

Options:
      --source <SOURCE>  `tool`, `umr` or `logcat`
      --level <LEVEL>    Lowest level: V, D, I, W, E or F
      --grep <GREP>      Only lines containing this text (or tag)
  -n, --lines <LINES>    How many recent lines [default: 100]
  -f, --follow           Keep printing new lines
      --json             One JSON object per line
  -h, --help             Print help
```

## umr mcp

```text
MCP server for an AI coding agent on stdin/stdout (the agent starts it; see `umr agent setup`)

Usage: umr mcp [OPTIONS]

Options:
      --client <CLIENT>  The agent client whose token and permissions to use [default: claude-code]
  -h, --help             Print help
```

## umr agent

```text
Connect AI coding agents: setup, clients, grants, audit log

Usage: umr agent <COMMAND>

Commands:
  setup         Connect Claude Code or Codex: creates its client, writes its MCP server entry and the agent instructions (shows the changes and asks first)
  add           Create an agent client with its own token (default scopes: observe, interact, control)
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

### umr agent setup

```text
Connect Claude Code or Codex: creates its client, writes its MCP server entry and the agent instructions (shows the changes and asks first)

Usage: umr agent setup [OPTIONS] <AGENT>

Arguments:
  <AGENT>  `claude-code` or `codex`

Options:
      --project <PROJECT>  Project folder (`.mcp.json`, skill / `AGENTS.md`); default: current folder
      --yes                Do not ask
      --dry-run            Only show what would change
  -h, --help               Print help
```

### umr agent add

```text
Create an agent client with its own token (default scopes: observe, interact, control)

Usage: umr agent add [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --scopes <SCOPES>  Comma-separated: observe, interact, control
  -h, --help             Print help
```

### umr agent list

```text
Agent clients, their scopes and grants

Usage: umr agent list [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### umr agent remove

```text
Remove an agent client (its token stops working)

Usage: umr agent remove <NAME>

Arguments:
  <NAME>  

Options:
  -h, --help  Print help
```

### umr agent grant

```text
Let an agent run a project folder (or `device:switch`) without asking each time

Usage: umr agent grant <NAME> <WHAT>

Arguments:
  <NAME>  
  <WHAT>  

Options:
  -h, --help  Print help
```

### umr agent ungrant

```text
Take a grant back

Usage: umr agent ungrant <NAME> <WHAT>

Arguments:
  <NAME>  
  <WHAT>  

Options:
  -h, --help  Print help
```

### umr agent audit

```text
What agents called (newest last)

Usage: umr agent audit [OPTIONS]

Options:
      --client <CLIENT>  Only this agent
  -n, --lines <LINES>    [default: 30]
      --json             
  -h, --help             Print help
```

### umr agent instructions

```text
Print the agent instructions (`skill` = Claude Code SKILL.md, `agents` = AGENTS.md section, `short` = MCP instructions)

Usage: umr agent instructions [OPTIONS]

Options:
      --format <FORMAT>  [default: skill]
  -h, --help             Print help
```

## umr approvals

```text
Requests from agents waiting for your answer (running a new project folder, switching the device)

Usage: umr approvals [COMMAND]

Commands:
  allow  Allow a request
  deny   Deny a request
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### umr approvals allow

```text
Allow a request

Usage: umr approvals allow [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --always  Remember it for this agent (records a grant)
  -h, --help    Print help
```

### umr approvals deny

```text
Deny a request

Usage: umr approvals deny <ID>

Arguments:
  <ID>  

Options:
  -h, --help  Print help
```

## umr reload

```text
Apply code changes to the running app: hot (default), restart or full

Usage: umr reload [OPTIONS] [KIND]

Arguments:
  [KIND]  

Options:
      --json  
  -h, --help  Print help
```

## umr adapter

```text
Framework adapters

Usage: umr adapter <COMMAND>

Commands:
  check  Run the adapter contract test kit live: start, every reload kind, stop
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### umr adapter check

```text
Run the adapter contract test kit live: start, every reload kind, stop

Usage: umr adapter check [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --profile <PROFILE>  
      --json               
  -h, --help               Print help
```

## umr session

```text
The current runtime session (device and app)

Usage: umr session [COMMAND]

Commands:
  status  State, recent events, supervised processes and ports (default)
  stop    Stop the session and its emulator
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### umr session status

```text
State, recent events, supervised processes and ports (default)

Usage: umr session status [OPTIONS]

Options:
      --json  
  -h, --help  Print help
```

### umr session stop

```text
Stop the session and its emulator

Usage: umr session stop

Options:
  -h, --help  Print help
```

## umr app

```text
Install, launch, stop or uninstall an app on the current device

Usage: umr app <COMMAND>

Commands:
  install    
  launch     
  stop       
  uninstall  
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### umr app install

```text
Usage: umr app install [OPTIONS] <APK>

Arguments:
  <APK>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### umr app launch

```text
Usage: umr app launch [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --activity <ACTIVITY>  Activity (`.Main` or `pkg/.Main`); default: the launcher activity
      --target <TARGET>      
  -h, --help                 Print help
```

### umr app stop

```text
Usage: umr app stop [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### umr app uninstall

```text
Usage: umr app uninstall [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## umr screenshot

```text
Save a screenshot of the current device (PNG)

Usage: umr screenshot [OPTIONS]

Options:
      --out <OUT>        Copy the PNG here as well
      --target <TARGET>  Device serial (default: the current session's device)
      --json             
  -h, --help             Print help
```

## umr ui

```text
Show the UI tree of the current screen (text, ids, bounds, clickable elements)

Usage: umr ui [OPTIONS]

Options:
      --target <TARGET>  
      --json             
  -h, --help             Print help
```

## umr input

```text
Send input to the current device

Usage: umr input [OPTIONS] <COMMAND>

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

### umr input tap

```text
Tap at device pixels

Usage: umr input tap [OPTIONS] <X> <Y>

Arguments:
  <X>  
  <Y>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### umr input swipe

```text
Swipe from (x1, y1) to (x2, y2)

Usage: umr input swipe [OPTIONS] <X1> <Y1> <X2> <Y2>

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

### umr input text

```text
Type text into the focused field

Usage: umr input text [OPTIONS] <TEXT>

Arguments:
  <TEXT>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

### umr input key

```text
Press a key: home, back, recents, enter, delete, tab, escape, menu, power, `volume_up`, `volume_down`, up, down, left, right

Usage: umr input key [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --target <TARGET>  
  -h, --help             Print help
```

## umr update

```text
Check for a newer version and install it (installed copies; portable copies get a link)

Usage: umr update [OPTIONS]

Options:
      --check  Only check, do not install
      --yes    Do not ask
      --json   
  -h, --help   Print help
```

## umr clean

```text
Delete the download cache and old logs

Usage: umr clean

Options:
  -h, --help  Print help
```

## umr admin

```text
Run one administrator action through a Windows UAC prompt: `enable-whpx` or `enable-long-paths`

Usage: umr admin [OPTIONS] <ACTION>

Arguments:
  <ACTION>  

Options:
      --yes   Do not ask for confirmation (Windows still shows its UAC prompt)
  -h, --help  Print help
```
