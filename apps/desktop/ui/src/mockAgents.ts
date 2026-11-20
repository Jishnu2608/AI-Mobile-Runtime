// Sample answers for the "Connect your agent" page in a plain browser (UI development,
// screenshots). Shapes match `tumr agent clients|setup|doctor --json`.
let pathOn = false;
const connected: Record<string, boolean> = { "claude-code": true };
const userScoped = new Set(["codex", "claude-desktop", "antigravity"]);
const project = String.raw`D:\Work\my-app`;

const clients: [string, string, boolean][] = [
  ["claude-code", "Claude Code", true],
  ["codex", "Codex", false],
  ["cursor", "Cursor", true],
  ["claude-desktop", "Claude Desktop", true],
  ["vscode", "VS Code (agent mode)", true],
  ["antigravity", "Antigravity", true],
];

const entryDiff = (id: string) =>
  [
    "+ {",
    '+   "mcpServers": {',
    '+     "tumr": {',
    '+       "type": "stdio",',
    String.raw`+       "command": "C:\\Users\\you\\AppData\\Local\\Toolchain Universal Mobile Runtime\\tumr.exe",`,
    `+       "args": ["mcp", "--client", "${id}"]`,
    "+     }",
    "+   }",
    "+ }",
  ].join("\n");

export async function mockAgent(req: { action: string; client?: string; remove?: boolean; scope?: string }): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 300));
  const id = req.client ?? "";
  if (req.action.startsWith("path_")) {
    if (req.action !== "path_status") pathOn = req.action === "path_add";
    return { on: pathOn, changed: req.action !== "path_status", dir: String.raw`C:\Users\you\AppData\Local\Toolchain Universal Mobile Runtime`, chosen: pathOn };
  }
  if (req.action === "clients") {
    return clients.map(([cid, name, installed]) => ({
      id: cid,
      name,
      installed,
      project_connected: !!connected[cid] && !userScoped.has(cid),
      user_connected: !!connected[cid] && userScoped.has(cid),
      project_config: cid === "claude-desktop" ? null : `${project}\\.config`,
      user_config: String.raw`C:\Users\you\.config`,
    }));
  }
  if (req.action === "plan" || req.action === "apply") {
    if (req.action === "apply") connected[id] = !req.remove;
    return {
      name: id,
      scope: req.scope ?? "project",
      remove: !!req.remove,
      client_exists: !!connected[id],
      applied: req.action === "apply",
      next_step: req.remove ? "Disconnected; restart the agent if it is open." : "Start the agent in your project; it lists the tumr server.",
      changes: [
        {
          path: `${project}\\.cursor\\mcp.json`,
          action: req.remove ? "delete" : "create",
          diff: req.remove ? entryDiff(id).replace(/^\+/gm, "-") : entryDiff(id),
        },
      ],
    };
  }
  return [
    {
      client: id,
      fixed: [],
      checks: [
        { result: "ok", what: `project scope: ${project}\\.cursor\\mcp.json`, fix: null },
        { result: "ok", what: `token for client '${id}'`, fix: null },
        { result: "ok", what: "no tool calls from this agent yet", fix: null },
        { result: "ok", what: "MCP handshake: 16 tools", fix: null },
      ],
    },
  ];
}
