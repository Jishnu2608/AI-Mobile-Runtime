# Agents (MCP)

The runtime is an MCP server, so any coding agent that speaks MCP can see and drive your running app. Install the runtime on your Windows PC, then connect each agent once. The agent starts `umr mcp` on the same PC; nothing goes through the internet.

## Connect an agent
| Agent | Command | What is written |
|---|---|---|
| Claude Code | `umr agent setup claude-code` | `<project>\.mcp.json` and the skill `<project>\.claude\skills\umr-runtime\SKILL.md`. With `--scope user`: `%USERPROFILE%\.claude.json` and `%USERPROFILE%\.claude\skills\umr-runtime\SKILL.md` (all projects) |
| Codex | `umr agent setup codex` | `%CODEX_HOME%\config.toml` (`[mcp_servers.umr]`) and a marked section in `<project>\AGENTS.md`. With `--scope project`: `<project>\.codex\config.toml` (Codex reads it in trusted projects) |
| Cursor | `umr agent setup cursor` | `<project>\.cursor\mcp.json` and the rule `<project>\.cursor\rules\umr-runtime.mdc`. With `--scope user`: `%USERPROFILE%\.cursor\mcp.json` |
| Claude Desktop | `umr agent setup claude-desktop` | Claude Desktop's `claude_desktop_config.json` (all chats); then quit Claude Desktop completely and start it again |
| VS Code (agent mode) | `umr agent setup vscode` | `<project>\.vscode\mcp.json` and `<project>\.github\instructions\umr-runtime.instructions.md`. With `--scope user`: the user `mcp.json` |
| Any other MCP client | `umr agent add <name>` | Nothing: add a stdio server yourself, command = the full path of `umr.exe`, arguments = `mcp --client <name>` |

- `<project>` is the current folder, or `--project <folder>`.
- Setup shows every change as a diff and asks first (`--yes` skips the question, `--dry-run` only shows). Other servers and settings in those files are kept, in their order. Running setup again changes nothing if the files are current.
- `--remove` disconnects an agent: it takes out only the `umr` entry and the runtime's instructions (files setup created are deleted, a shared file goes back to how it was) and revokes the agent's token.
- `--print` prints the entry without writing anything, for setting a client up by hand.
- No secret goes into any agent's files: each agent's token stays in `%USERPROFILE%\.umr\agents\`.
- `umr agent instructions` prints the instructions agents get.

### In the desktop app
The setup wizard's last step, **Agent**, lists the coding agents on your PC. For each one: **Connect** shows the files it will change as a diff and writes them when you press **Apply**; **Check** runs the doctor below; **Disconnect** takes the runtime out again. The page runs the same `umr agent` commands as the command line.

### One-click install links
`umr agent link cursor` and `umr agent link vscode` print an install link made for your PC (it contains the path of your `umr.exe`). Open it, and Cursor or VS Code shows the `umr` server and asks before adding it. The link adds only the server; `umr agent setup` also adds the agent instructions. There are no install buttons on the website, because the path differs on every PC.

## Check a connection
- `umr agent clients` lists the coding agents found on your PC and whether each one is connected (`--json` adds the config file paths and the exact entries).
- `umr agent doctor [agent]` checks every connected agent end to end: the config entry, the `umr.exe` it points at, the agent's token, a real MCP handshake (the same start-up the agent does) and the agent's last tool call. Each problem comes with the command that fixes it.
- `umr agent doctor --fix` points entries back at the current `umr.exe`, for example after you moved the runtime or used the portable ZIP from another folder.

| Doctor says | What to do |
|---|---|
| no `umr` server in its config | `umr agent setup <agent>` |
| program not found / points at another umr.exe | `umr agent doctor <agent> --fix` |
| no token for client | `umr agent setup <agent>` (or `umr agent add <name>`) |
| is not plain JSON | The file has comments or a syntax error. Fix it in the editor (VS Code allows comments in `mcp.json`; the runtime does not edit such a file), then run setup again |
| MCP handshake failed | `umr status`, then `umr start`; run the doctor again |

The claude.ai website and mobile app cannot use the runtime: their connectors run in Anthropic's cloud and cannot reach a program on your PC.

## Tools
| Tool | What it does |
|---|---|
| `runtime_status` | What runs and what changed since the last call (≤ 1 KB). Agents call it after every edit |
| `get_diagnostics`, `get_diagnostic` | Problems by category with origin (your code / environment), file and line; one problem with its stack |
| `get_logs` | Recent app log (logcat), tool output and progress |
| `screenshot`, `ui_tree` | The screen (scaled image) and its elements with coordinates |
| `tap`, `swipe`, `type_text`, `press_key`, `rotate` | Input in device pixels |
| `reload`, `run_project`, `stop_app` | Hot / restart / full reload; run a project folder; stop the app |
| `switch_device`, `list_device_profiles` | Change the phone size |

## Permissions
- Each agent has its own client and token (`umr agent list`). Agents get **observe, interact and control**, never admin: they cannot install software, trust folders or manage other clients.
- The first time an agent runs a project folder, or whenever it switches the device, the runtime **asks you**: a banner in the preview window (Allow / Always / Deny) or `umr approvals`. "Always" remembers it for that agent (`umr agent grant|ungrant`).
- Every call an agent makes is recorded: `umr agent audit [--client <name>]`.
- Everything agents read is sanitised: tokens, keys, passwords, e-mail addresses and your user name are masked.
- An agent that may run shell commands on your PC could also run `umr` directly, outside these rules. The instructions tell agents not to; if that matters to you, do not allow your agent to run shell commands.

Closing or killing the agent never stops the device or the app.
