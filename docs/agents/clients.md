# Coding-agent clients — how each one loads the runtime's MCP server

Checked against each client's own documentation in November 2026. The same
facts are in code, in `apps/cli/src/agent_clients.rs` (one entry per client); `tumr agent clients`
shows which of them are installed and connected on a PC.

Every client starts the same local program over stdio:

```text
command: <full path of tumr.exe>
args:    mcp --client <client id>
```

The config entry holds no secret. The client's token stays in `%USERPROFILE%\.tumr\agents\<id>.token`
(owner-only), and scopes, approvals and the audit log apply as for every agent (Phase 6).

## Summary

| Client (id) | Project config | User config | Server list key | Instructions | Setup today |
|---|---|---|---|---|---|
| Claude Code (`claude-code`) | `<project>\.mcp.json` | `%USERPROFILE%\.claude.json` | `mcpServers`, `"type": "stdio"` | `.claude\skills\tumr-runtime\SKILL.md` | `tumr agent setup claude-code` |
| Codex (`codex`) | `<project>\.codex\config.toml` (trusted projects only) | `%CODEX_HOME%\config.toml` (default `%USERPROFILE%\.codex`) | `[mcp_servers.tumr]` with `command` | marked section in `<project>\AGENTS.md` | `tumr agent setup codex` |
| Cursor (`cursor`) | `<project>\.cursor\mcp.json` | `%USERPROFILE%\.cursor\mcp.json` | `mcpServers`, `"type": "stdio"` | `.cursor\rules\tumr-runtime.mdc` | `tumr agent setup cursor` |
| Claude Desktop (`claude-desktop`) | — | `%LOCALAPPDATA%\Claude-Data\<package>\claude_desktop_config.json` on current builds (below) | `mcpServers` | MCP `instructions` only | `tumr agent setup claude-desktop` |
| VS Code agent mode (`vscode`) | `<project>\.vscode\mcp.json` | `%APPDATA%\Code\User\mcp.json` | `servers`, `"type"` required | `.github\instructions\tumr-runtime.instructions.md` | `tumr agent setup vscode` |
| Antigravity (`antigravity`) | `<project>\.agents\mcp_config.json` (CLI and IDE) | `%USERPROFILE%\.gemini\config\mcp_config.json` (App, CLI, IDE) | `mcpServers`, no `type` (`command` = stdio) | `.agents\rules\tumr-runtime.md` / `~\.gemini\config\rules\tumr-runtime.md` (front matter `trigger: always_on`) | `tumr agent setup antigravity` |

## Per client

### Claude Code
- Three scopes. **Local** (default of `claude mcp add`): private, one project, stored in `~/.claude.json` under that project. **Project**: `.mcp.json` in the project root, shared through version control; Claude Code asks once to approve it (`/mcp` turns it on later). **User**: all projects, private, in `~/.claude.json`.
- If one name exists in several scopes, local beats project beats user; the winning entry is used whole (fields are not merged).
- Claude Code does not read `~/.claude/.mcp.json`.
- `tumr agent setup claude-code` writes the project scope by default; `--scope user` writes the user scope.
- Source: [Claude Code MCP docs](https://code.claude.com/docs/en/mcp).

### Codex
- `[mcp_servers.<name>]` tables in `~/.codex/config.toml`; a project can add `.codex/config.toml`, read only in trusted projects.
- `command` selects stdio, `url` selects HTTP; mixing them in one table is rejected. Optional `args`, `env`, `startup_timeout_sec`, `tool_timeout_sec` (setup writes 30 s and 600 s, because builds can be long).
- `codex mcp add <name> -- <command>` does the same from the command line.
- Source: [Codex MCP docs](https://developers.openai.com/codex/mcp).

### Cursor
- `.cursor/mcp.json` in the project, `~/.cursor/mcp.json` for all projects; top-level `mcpServers`.
- Stdio entry: `type: "stdio"`, `command` (on `PATH` or a full path), optional `args`, `env`, `envFile`. Variables `${env:NAME}`, `${userHome}`, `${workspaceFolder}` are expanded.
- Rules: `.mdc` files in `.cursor/rules` with front matter (`description`, `globs`, `alwaysApply`); `alwaysApply: true` loads the rule in every chat. A plain `.md` file there is ignored.
- One-click install links exist (`cursor://anysphere.cursor-deeplink/mcp/install?name=…&config=<base64>`); `tumr agent link cursor` makes one for your PC, because it carries the full path of your `tumr.exe`.
- Sources: [Cursor MCP docs](https://cursor.com/docs/mcp), [Cursor rules](https://cursor.com/docs/rules), [install links](https://cursor.com/docs/context/mcp/install-links).

### Claude Desktop
- Local servers go in `claude_desktop_config.json` under `mcpServers` (Settings → Developer → Edit Config). Restart the app after a change. Remote servers in this file are ignored; they are added as connectors in the app instead.
- **Where the file is on Windows (checked on a real install, Claude 5.11492, 2026-11-14).** The packaged app keeps its settings in `%LOCALAPPDATA%\Claude-Data\<package family, e.g. claude_pzs8sxrjxfjjc>\claude_desktop_config.json`, and leaves a `.user-data-left.<package>` marker in the folders it used before: `%APPDATA%\Claude` and the MSIX package's `LocalCache\Roaming\Claude` (the location a public bug report describes for older builds). The runtime picks the first candidate that already has the file, otherwise the first one without such a marker, otherwise `%APPDATA%\Claude`.
- The file also holds the app's own settings (`preferences`, …); setup adds `mcpServers` and keeps everything else in its order.
- **Quit Claude Desktop before connecting** (also from the tray). A running Claude Desktop writes its settings back to this file and drops the `tumr` entry (seen live on 2026-11-19: the entry was gone minutes after Connect). `tumr agent doctor claude-desktop` shows whether it is still there.
- User-wide: every project the agent opens still needs your approval the first time.
- No instructions file: the agent gets the MCP `instructions` sent at start-up.
- Sources: [Claude Code issue on the MSIX path](https://github.com/anthropics/claude-code/issues/26073), [Claude help: custom connectors](https://support.anthropic.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers).

### VS Code (agent mode)
- `.vscode/mcp.json` in the workspace or `mcp.json` in the user profile; top-level `servers`; `type` is required (`"stdio"`), with `command`, optional `args`, `cwd` (defaults to the workspace folder), `env`.
- The newer Copilot harness also reads a workspace-root `.mcp.json` with `mcpServers` — the same file Claude Code uses, so a Claude Code project setup may show up there too.
- Instructions: `.github/instructions/*.instructions.md` (optional `applyTo`, `description`), or `.github/copilot-instructions.md` / `AGENTS.md`.
- Sources: [VS Code MCP configuration reference](https://code.visualstudio.com/docs/agents/reference/mcp-configuration), [custom instructions](https://code.visualstudio.com/docs/agent-customization/custom-instructions).

### Antigravity
- Google's agent app, CLI and IDE share one MCP file for all projects: `~/.gemini/config/mcp_config.json`, top-level `mcpServers`. A workspace can add `.agents/mcp_config.json`, which the CLI and the IDE read; the App shows its own file under Customizations → Installed → MCP Servers → Copy Config File Path. That is why setup writes the user file by default.
- A local server is `command` plus optional `args` and `env`; there is no `type` key (`serverUrl` would make it a remote server).
- Rules: `.md` files in `.agents/rules/` (workspace) or `~/.gemini/config/rules/` (all projects). Each needs YAML front matter with a valid `trigger` (`always_on`, `model_decision`, `glob`, `manual`), or it is silently skipped. `AGENTS.md` / `GEMINI.md` are read too.
- Sources: [Antigravity MCP](https://antigravity.google/docs/mcp), [rules](https://antigravity.google/docs/rules), [hooks](https://antigravity.google/docs/hooks).

## Not covered here
claude.ai web and mobile connectors: they connect from Anthropic's cloud to a public HTTPS endpoint and cannot reach a program on your PC.

## Setup, removal and checks
`tumr agent setup <id> [--scope project|user] [--remove] [--print]` and `tumr agent doctor [id] [--fix]` work for all six clients. Every entry holds the absolute path of `tumr.exe` (DEC-055); after each install the installer runs `tumr agent repoint`, which points all-projects entries that start another `tumr.exe` at the new one (project files are left alone; `tumr agent doctor --fix` repoints those). Default scope: project for Claude Code, Cursor and VS Code; user for Codex, Claude Desktop and Antigravity.

## Stop hooks
`tumr agent setup` also registers `tumr agent hook <client>` as the agent's stop hook, so build errors and crashes left after the agent's turn go back to it (user guide: `docs/user/agents.md`, *Problems go back to the agent*). Formats checked against each client's documentation on 2026-11-15:

| Client | File (project / user) | Entry | Keeps the agent going with |
|---|---|---|---|
| Claude Code | `.claude/settings.local.json` / `~/.claude/settings.json` | `hooks.Stop[].hooks[]`: `{type: "command", command: <tumr.exe>, args: ["agent","hook","claude-code"], timeout: 150}` (exec form, no shell) | stdout `{"decision":"block","reason":…}` |
| Codex | `.codex/hooks.json` / `%CODEX_HOME%\hooks.json` | `hooks.Stop[].hooks[]`: `{type: "command", command: "\"<tumr.exe>\" agent hook codex", timeout: 150}`; new hooks must be trusted once (`/hooks`) | stdout `{"decision":"block","reason":…}` (the reason becomes the next prompt) |
| Cursor | `.cursor/hooks.json` / `~/.cursor/hooks.json` | `{version: 1, hooks: {stop: [{command: "& '<tumr.exe>' agent hook cursor", timeout: 150, loop_limit: 3}]}}` (PowerShell on Windows) | stdout `{"followup_message":…}` |
| Antigravity | `.agents/hooks.json` / `~/.gemini/config/hooks.json` | a named entry `tumr: {Stop: [{type: "command", command: "\"<tumr.exe>\" agent hook antigravity", timeout: 150}]}` (checked 2026-11-19) | stdout `{"decision":"continue","reason":…}` (the reason is added as a system message); `{"decision":"stop"}` lets it stop |

**Codex app (ChatGPT desktop) on Windows:** with its Windows sandbox on (`[windows] sandbox = "elevated"` in `~/.codex/config.toml`), Codex runs hooks as its own sandbox account (`CodexSandboxOnline`). The runtime only lets your own Windows account in, so that hook cannot report and Codex stops as usual. This is on purpose: an agent's sandbox account does not get into your runtime. Codex's `tumr` tools work as normal (Codex starts them as you), so ask it to check the app's problems (`runtime_status`) after a change, or use **Send to agent** in the preview.

Some Cursor versions on Windows are reported to ignore `followup_message`; there the agent stops as usual and the problems stay in `tumr problems`. A forum report says Antigravity IDE 1.107 on Windows did not run Stop hooks from `.agents/hooks.json`; the same fallback applies.

## Install links
`tumr agent link cursor|vscode [--json]` prints a link made on the PC, because it carries that PC's `tumr.exe` path (which is why the website has no install buttons):
- Cursor: `cursor://anysphere.cursor-deeplink/mcp/install?name=tumr&config=<URL-encoded base64 of the server entry>` ([docs](https://cursor.com/docs/mcp/install-links)).
- VS Code: `vscode:mcp/install?<URL-encoded JSON entry with "name": "tumr">` ([docs](https://code.visualstudio.com/api/extension-guides/ai/mcp)).
The link creates the agent's client first; it adds only the server entry, not the instructions file. Claude Code, Codex and Claude Desktop have no install links;

## Desktop app
The wizard's last step lists these clients and connects, checks or disconnects each one by running `tumr agent clients|setup|doctor --json` (the installed `tumr.exe` next to the app, fixed argument lists, no shell).
