# Agents (MCP)

The runtime is an MCP server, so any coding agent that speaks MCP can see and drive your running app. Install the runtime on your Windows PC, then connect each agent once. The agent starts `tumr mcp` on the same PC; nothing goes through the internet.

## Install a plugin (the easy way)
Each agent has a plugin that brings the `tumr` MCP server, the runtime's instructions and the error loop in one install. Install the runtime first and open it once; the plugin finds `tumr.exe` in the default install folder, through the `TUMR_EXE` environment variable, or on `PATH`. On first use the plugin connects the agent by itself, and the home screen says "<agent> connected itself through its plugin".

| Agent | Install |
|---|---|
| Claude Code | `/plugin install tumr --marketplace Jishnu2608/TUMR` (or `claude plugin marketplace add Jishnu2608/TUMR`, then `claude plugin install tumr@tumr`) |
| Codex | `codex plugin marketplace add Jishnu2608/TUMR`, then install **tumr** in the Codex app's Plugins directory |
| Cursor | From the Cursor Marketplace once listed; until then copy `plugins\cursor` from the repository to `%USERPROFILE%\.cursor\plugins\local\tumr` and run **Developer: Reload Window** |
| Antigravity | `agy plugin install <path to plugins\antigravity>` |
| VS Code (Copilot) | The VS Code extension `tumr.tumr-vscode`, then **Connect Copilot (agent mode)** |
| Claude Desktop | Open the `tumr-mcp_<version>_win32-x64.mcpb` file from the release page |

When an agent starts the app (`run_project`), the phone opens in its own window on the left of the screen. If you move it, it stays where you put it. In VS Code the Mobile Preview opens to the left of the code.

To disconnect a plugin, remove the agent on the runtime's Agents page (or `tumr agent remove <agent>`): the plugin then stays disconnected until you connect the agent again there or with `tumr agent setup`.

## Connect an agent without a plugin
| Agent | Command | What is written |
|---|---|---|
| Claude Code | `tumr agent setup claude-code` | `<project>\.mcp.json`, the skill `<project>\.claude\skills\tumr-runtime\SKILL.md` and the stop hook in `<project>\.claude\settings.local.json`. With `--scope user`: `%USERPROFILE%\.claude.json`, `%USERPROFILE%\.claude\skills\tumr-runtime\SKILL.md` and `%USERPROFILE%\.claude\settings.json` (all projects) |
| Codex | `tumr agent setup codex` | `%CODEX_HOME%\config.toml` (`[mcp_servers.tumr]`), the stop hook in `%CODEX_HOME%\hooks.json` and a marked section in `<project>\AGENTS.md`. With `--scope project`: `<project>\.codex\config.toml` and `<project>\.codex\hooks.json` (Codex reads them in trusted projects) |
| Cursor | `tumr agent setup cursor` | `<project>\.cursor\mcp.json`, the rule `<project>\.cursor\rules\tumr-runtime.mdc` and the stop hook in `<project>\.cursor\hooks.json`. With `--scope user`: `%USERPROFILE%\.cursor\mcp.json` and `%USERPROFILE%\.cursor\hooks.json` |
| Claude Desktop | `tumr agent setup claude-desktop` | Claude Desktop's `claude_desktop_config.json` (all chats); then quit Claude Desktop completely and start it again |
| VS Code (agent mode) | `tumr agent setup vscode` | `<project>\.vscode\mcp.json`, `<project>\.github\instructions\tumr-runtime.instructions.md` and the stop hook in `<project>\.github\hooks\tumr.json`. With `--scope user`: the user `mcp.json` and `%USERPROFILE%\.copilot\hooks\tumr.json` |
| Antigravity | `tumr agent setup antigravity` | `%USERPROFILE%\.gemini\config\mcp_config.json` (App, CLI and IDE), the rule `%USERPROFILE%\.gemini\config\rules\tumr-runtime.md` and the stop hook in `%USERPROFILE%\.gemini\config\hooks.json` (all projects). With `--scope project`: `<project>\.agents\mcp_config.json`, `<project>\.agents\rules\tumr-runtime.md` and `<project>\.agents\hooks.json` (read by the Antigravity CLI and IDE). Then refresh `tumr` under MCP Servers and start a new conversation |
| Any other MCP client | `tumr agent add <name>` | Nothing: add a stdio server yourself, command = the full path of `tumr.exe`, arguments = `mcp --client <name>` |

- `<project>` is the current folder, or `--project <folder>`.
- Setup shows every change as a diff and asks first (`--yes` skips the question, `--dry-run` only shows). Other servers and settings in those files are kept, in their order. Running setup again changes nothing if the files are current.
- `--remove` disconnects an agent: it takes out only the `tumr` entry and the runtime's instructions (files setup created are deleted, a shared file goes back to how it was) and revokes the agent's token.
- `--print` prints the entry without writing anything, for setting a client up by hand.
- No secret goes into any agent's files: each agent's token stays in `%USERPROFILE%\.tumr\agents\`.
- `tumr agent instructions` prints the instructions agents get.

### Problems go back to the agent
For Claude Code, Codex, Cursor, Antigravity and GitHub Copilot in VS Code, setup (and each plugin) also adds a **stop hook**: `tumr agent hook <agent>` runs each time the agent ends its turn. It works the same for every framework the runtime runs (Kotlin, Java, Dart/Flutter, React Native/Expo):

1. It waits until the rebuild or reload started by the agent's last saves has finished. The wait depends on what runs: with nothing building it answers within a second or two; while Gradle (or Flutter, or Metro) builds, it waits for the build, however long, for up to about ten minutes.
2. If the app does not build, or a new runtime error or crash appeared, it hands the agent a short report — kind, `file:line`, message, and the fingerprint for `get_diagnostic` — and the agent keeps working on it instead of stopping. Claude Code, Codex and Copilot get it as the reason the stop was blocked; Cursor as a follow-up message; Antigravity as a reason to continue.
3. When everything builds and runs, the agent stops as usual.

This repeats after every task until the app has no problems. It also works when the app has never built: if the first build fails, the runtime keeps watching the folder and builds again on the next save. It stops by itself only when the **same** problem comes back five times in a row: then the agent stops and the home screen says "Stopped sending <agent> back", so you can look at it yourself or send it again with **Send to agent**. A build that is still running after ten minutes sends the agent back to wait for it with the `wait_for_app` tool (at most three times in a row).

The hook never stands in the way: when nothing of that folder runs or the Runtime Core is not started, the agent simply stops. Problems of the toolchain or the device are not sent to the agent; they are shown to you. The hook uses the agent's own token (read-only scope) and never the administrator token. GitHub Copilot's hook goes to `%USERPROFILE%\.copilot\hooks\tumr.json` (all projects) or `<project>\.github\hooks\tumr.json`.

**During the turn (Claude Code only).** The stop hook speaks when the agent ends its turn. Claude Code also gets an **after-edit hook** (`tumr agent hook claude-code --after-edit`, on its `PostToolUse` event for the edit tools): after each file edit it passes on problems the running app already shows from earlier saves, as extra context, without waiting for a build and without stopping the agent. Each set of problems is said once. For Codex and Cursor the runtime uses only the end-of-turn hook (a mid-turn report there is not built or checked yet), so for them the report comes when they finish; ask them to call `runtime_status` after larger changes if you want it sooner. Older setups get a doctor warning and pick the hook up with `tumr agent setup claude-code`.

Codex asks you to trust new hooks once (`/hooks` in Codex). **Codex app (ChatGPT desktop) on Windows:** with its Windows sandbox on (`[windows] sandbox = "elevated"` in `~/.codex/config.toml`), Codex runs hooks as its own sandbox account (`CodexSandboxOnline`). The runtime only lets your own Windows account in, so that hook cannot report and Codex stops as usual. This is on purpose: an agent's sandbox account does not get into your runtime. Codex's `tumr` tools work as normal (Codex starts them as you), so ask it to check the app's problems (`runtime_status`) after a change, or use **Send to agent** in the preview. The Claude Code hook goes to `settings.local.json`, which Claude Code keeps out of git, because it holds this PC's `tumr.exe` path.

### In the desktop app
The setup wizard's last step, **Agent**, lists the coding agents on your PC. For each one: **Connect** shows the files it will change as a diff and writes them when you press **Apply**; **Check** runs the doctor below; **Disconnect** takes the runtime out again. The page runs the same `tumr agent` commands as the command line.

### One-click install links
`tumr agent link cursor` and `tumr agent link vscode` print an install link made for your PC (it contains the path of your `tumr.exe`). Open it, and Cursor or VS Code shows the `tumr` server and asks before adding it. The link adds only the server; `tumr agent setup` also adds the agent instructions. There are no install buttons on the website, because the path differs on every PC.

## Check a connection
- `tumr agent clients` lists the coding agents found on your PC and whether each one is connected (`--json` adds the config file paths and the exact entries).
- `tumr agent doctor [agent]` checks every connected agent end to end: the config entry, the `tumr.exe` it points at, the agent's token, a real MCP handshake (the same start-up the agent does) and the agent's last tool call. Each problem comes with the command that fixes it.
- `tumr agent doctor --fix` points entries back at the current `tumr.exe`, for example after you moved the runtime or used the portable ZIP from another folder.

| Doctor says | What to do |
|---|---|
| no `tumr` server in its config | `tumr agent setup <agent>` |
| program not found / points at another tumr.exe | `tumr agent doctor <agent> --fix` |
| no token for client | `tumr agent setup <agent>` (or `tumr agent add <name>`) |
| is not plain JSON | The file has comments or a syntax error. Fix it in the editor (VS Code allows comments in `mcp.json`; the runtime does not edit such a file), then run setup again |
| MCP handshake failed | `tumr status`, then `tumr start`; run the doctor again |

The claude.ai website and mobile app cannot use the runtime: their connectors run in Anthropic's cloud and cannot reach a program on your PC.

## Tools
| Tool | What it does |
|---|---|
| `runtime_status` | What runs and what changed since the last call (≤ 1 KB). Agents call it after every edit |
| `get_diagnostics`, `get_diagnostic` | Problems by category with origin (your code / environment), file and line; one problem with its stack |
| `get_logs` | Recent app log (logcat), tool output and progress |
| `screenshot`, `ui_tree` | The screen (scaled image) and its elements with coordinates |
| `tap`, `swipe`, `type_text`, `press_key`, `rotate` | Input in device pixels |
| `fingerprint` | Touch the fingerprint sensor (enrolled finger 1): unlocks the lock screen or passes the app's `BiometricPrompt`; says whether the phone is still locked |
| `reload`, `run_project`, `stop_app` | Hot / restart / full reload; run a project folder; stop the app |
| `switch_device`, `list_device_profiles` | Change the phone size |

## Permissions
- Each agent has its own client and token (`tumr agent list`). Agents get **observe, interact and control**, never admin: they cannot install software, trust folders or manage other clients.
- The first time an agent runs a project folder, or whenever it switches the device, the runtime **asks you**: a banner in the preview window (Allow / Always / Deny) or `tumr approvals`. "Always" remembers it for that agent (`tumr agent grant|ungrant`).
- Every call an agent makes is recorded: `tumr agent audit [--client <name>]`.
- Everything agents read is sanitised: tokens, keys, passwords, e-mail addresses and your user name are masked.
- An agent that may run shell commands on your PC could also run `tumr` directly, outside these rules. The instructions tell agents not to; if that matters to you, do not allow your agent to run shell commands.

Closing or killing the agent never stops the device or the app.
