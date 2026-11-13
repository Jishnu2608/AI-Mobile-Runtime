# Agents (MCP)

The runtime is an MCP server, so any coding agent that speaks MCP can see and drive your running app. Claude Code and Codex are set up for you; others need one entry in their MCP configuration.

## Setup
| Agent | Command | What is written |
|---|---|---|
| Claude Code | `umr agent setup claude-code [--project <folder>]` | `<folder>\.mcp.json` (the `umr` server; other servers kept) and `<folder>\.claude\skills\umr-runtime\SKILL.md` |
| Codex | `umr agent setup codex [--project <folder>]` | `%CODEX_HOME%\config.toml` (`[mcp_servers.umr]`) and a marked section in `<folder>\AGENTS.md` |
| Other MCP clients | `umr agent add <name>` | Add a stdio server: command = the full path of `umr.exe`, arguments = `mcp --client <name>` |

`--dry-run` shows the changes without writing; without `--yes` it asks first. Running setup again changes nothing if the files are current. `umr agent instructions` prints the instructions agents get.

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
