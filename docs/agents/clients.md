# Coding-agent clients — how each one loads the runtime's MCP server

Phase 10.1 (CON-001). Checked against each client's own documentation on **2026-11-14**. The same
facts are in code, in `apps/cli/src/agent_clients.rs` (one entry per client); `umr agent clients`
shows which of them are installed and connected on a PC.

Every client starts the same local program over stdio:

```text
command: <full path of umr.exe>
args:    mcp --client <client id>
```

The config entry holds no secret. The client's token stays in `%USERPROFILE%\.umr\agents\<id>.token`
(owner-only), and scopes, approvals and the audit log apply as for every agent (Phase 6).

## Summary

| Client (id) | Project config | User config | Server list key | Instructions | Setup today |
|---|---|---|---|---|---|
| Claude Code (`claude-code`) | `<project>\.mcp.json` | `%USERPROFILE%\.claude.json` | `mcpServers`, `"type": "stdio"` | `.claude\skills\umr-runtime\SKILL.md` | `umr agent setup claude-code` |
| Codex (`codex`) | `<project>\.codex\config.toml` (trusted projects only) | `%CODEX_HOME%\config.toml` (default `%USERPROFILE%\.codex`) | `[mcp_servers.umr]` with `command` | marked section in `<project>\AGENTS.md` | `umr agent setup codex` |
| Cursor (`cursor`) | `<project>\.cursor\mcp.json` | `%USERPROFILE%\.cursor\mcp.json` | `mcpServers`, `"type": "stdio"` | `.cursor\rules\umr-runtime.mdc` | `umr agent setup cursor` |
| Claude Desktop (`claude-desktop`) | — | `%LOCALAPPDATA%\Claude-Data\<package>\claude_desktop_config.json` on current builds (below) | `mcpServers` | MCP `instructions` only | `umr agent setup claude-desktop` |
| VS Code agent mode (`vscode`) | `<project>\.vscode\mcp.json` | `%APPDATA%\Code\User\mcp.json` | `servers`, `"type"` required | `.github\instructions\umr-runtime.instructions.md` | `umr agent setup vscode` |

## Per client

### Claude Code
- Three scopes. **Local** (default of `claude mcp add`): private, one project, stored in `~/.claude.json` under that project. **Project**: `.mcp.json` in the project root, shared through version control; Claude Code asks once to approve it (`/mcp` turns it on later). **User**: all projects, private, in `~/.claude.json`.
- If one name exists in several scopes, local beats project beats user; the winning entry is used whole (fields are not merged).
- Claude Code does not read `~/.claude/.mcp.json`.
- `umr agent setup claude-code` writes the project scope today; a user-scope option is DEC-031 / CON-002.
- Source: [Claude Code MCP docs](https://code.claude.com/docs/en/mcp).

### Codex
- `[mcp_servers.<name>]` tables in `~/.codex/config.toml`; a project can add `.codex/config.toml`, read only in trusted projects.
- `command` selects stdio, `url` selects HTTP; mixing them in one table is rejected. Optional `args`, `env`, `startup_timeout_sec`, `tool_timeout_sec` (setup writes 30 s and 600 s, because builds can be long).
- `codex mcp add <name> -- <command>` does the same from the command line.
- Not yet run live with the runtime (MCP-007 / CON-007).
- Source: [Codex MCP docs](https://developers.openai.com/codex/mcp).

### Cursor
- `.cursor/mcp.json` in the project, `~/.cursor/mcp.json` for all projects; top-level `mcpServers`.
- Stdio entry: `type: "stdio"`, `command` (on `PATH` or a full path), optional `args`, `env`, `envFile`. Variables `${env:NAME}`, `${userHome}`, `${workspaceFolder}` are expanded.
- Rules: `.mdc` files in `.cursor/rules` with front matter (`description`, `globs`, `alwaysApply`); `alwaysApply: true` loads the rule in every chat. A plain `.md` file there is ignored.
- One-click install links exist (`cursor://anysphere.cursor-deeplink/mcp/install?name=…&config=<base64>`) — CON-006, blocked by DEC-055 (the full path differs per PC).
- Sources: [Cursor MCP docs](https://cursor.com/docs/mcp), [Cursor rules](https://cursor.com/docs/rules), [install links](https://cursor.com/docs/context/mcp/install-links).

### Claude Desktop
- Local servers go in `claude_desktop_config.json` under `mcpServers` (Settings → Developer → Edit Config). Restart the app after a change. Remote servers in this file are ignored; they are added as connectors in the app instead.
- **Where the file is on Windows (checked on a real install, Claude 5.11492, 2026-11-14).** The packaged app keeps its settings in `%LOCALAPPDATA%\Claude-Data\<package family, e.g. claude_pzs8sxrjxfjjc>\claude_desktop_config.json`, and leaves a `.user-data-left.<package>` marker in the folders it used before: `%APPDATA%\Claude` and the MSIX package's `LocalCache\Roaming\Claude` (the location a public bug report describes for older builds). The runtime picks the first candidate that already has the file, otherwise the first one without such a marker, otherwise `%APPDATA%\Claude`. The live check that Claude Desktop loads the server is CON-007.
- The file also holds the app's own settings (`preferences`, …); setup adds `mcpServers` and keeps everything else in its order.
- User-wide: every project the agent opens still needs your approval the first time.
- No instructions file: the agent gets the MCP `instructions` sent at start-up.
- Sources: [Claude Code issue on the MSIX path](https://github.com/anthropics/claude-code/issues/26073), [Claude help: custom connectors](https://support.anthropic.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers).

### VS Code (agent mode)
- `.vscode/mcp.json` in the workspace or `mcp.json` in the user profile; top-level `servers`; `type` is required (`"stdio"`), with `command`, optional `args`, `cwd` (defaults to the workspace folder), `env`.
- The newer Copilot harness also reads a workspace-root `.mcp.json` with `mcpServers` — the same file Claude Code uses, so a Claude Code project setup may show up there too.
- Instructions: `.github/instructions/*.instructions.md` (optional `applyTo`, `description`), or `.github/copilot-instructions.md` / `AGENTS.md`.
- Sources: [VS Code MCP configuration reference](https://code.visualstudio.com/docs/agents/reference/mcp-configuration), [custom instructions](https://code.visualstudio.com/docs/agent-customization/custom-instructions).

## Not covered here
claude.ai web and mobile connectors: they connect from Anthropic's cloud to a public HTTPS endpoint and cannot reach a program on your PC. A hosted runtime for them is decision DEC-053, outside Phase 10.

## Setup, removal and checks
`umr agent setup <id> [--scope project|user] [--remove] [--print]` and `umr agent doctor [id] [--fix]` work for all five clients (CON-002…004). Default scope: project for Claude Code, Cursor and VS Code; user for Codex and Claude Desktop.

## Stop hooks (HOOK-001)
`umr agent setup` also registers `umr agent hook <client>` as the agent's stop hook, so build errors and crashes left after the agent's turn go back to it (user guide: `docs/user/agents.md`, *Problems go back to the agent*). Formats checked against each client's documentation on 2026-11-15:

| Client | File (project / user) | Entry | Keeps the agent going with |
|---|---|---|---|
| Claude Code | `.claude/settings.local.json` / `~/.claude/settings.json` | `hooks.Stop[].hooks[]`: `{type: "command", command: <umr.exe>, args: ["agent","hook","claude-code"], timeout: 150}` (exec form, no shell) | stdout `{"decision":"block","reason":…}` |
| Codex | `.codex/hooks.json` / `%CODEX_HOME%\hooks.json` | `hooks.Stop[].hooks[]`: `{type: "command", command: "\"<umr.exe>\" agent hook codex", timeout: 150}`; new hooks must be trusted once (`/hooks`) | stdout `{"decision":"block","reason":…}` (the reason becomes the next prompt) |
| Cursor | `.cursor/hooks.json` / `~/.cursor/hooks.json` | `{version: 1, hooks: {stop: [{command: "& '<umr.exe>' agent hook cursor", timeout: 150, loop_limit: 3}]}}` (PowerShell on Windows) | stdout `{"followup_message":…}` |

Live: Claude Code 2.1.310 passed 2026-11-15 (Kotlin demo, seeded compile error). Codex and Cursor: not run yet (HOOK-002); Cursor has known forum reports of `followup_message` being ignored on Windows in some versions.

## Install links (CON-006)
`umr agent link cursor|vscode [--json]` prints a link made on the PC, because it carries that PC's `umr.exe` path (the reason the website has no install buttons, DEC-055):
- Cursor: `cursor://anysphere.cursor-deeplink/mcp/install?name=umr&config=<URL-encoded base64 of the server entry>` ([docs](https://cursor.com/docs/mcp/install-links)).
- VS Code: `vscode:mcp/install?<URL-encoded JSON entry with "name": "umr">` ([docs](https://code.visualstudio.com/api/extension-guides/ai/mcp)).
The link creates the agent's client first; it adds only the server entry, not the instructions file. Claude Code, Codex and Claude Desktop have no install links; a Claude Code plugin is not built (it needs a public name and a place to publish it, W1 / W5).

## Desktop app (CON-005)
The wizard's last step lists these clients and connects, checks or disconnects each one by running `umr agent clients|setup|doctor --json` (the installed `umr.exe` next to the app, fixed argument lists, no shell).

## Still to confirm live (CON-007)
Whether each client shows screenshot results as images, its tool time-out with long builds, and that Claude Desktop, Cursor and VS Code load the server from the files setup writes (the files, the program path and an MCP handshake are already checked by `umr agent doctor`).
