// Copies the repository's own documents into content/generated so the site renders the same text
// the repository ships (dev_plan §11.1, principle 4: docs live next to the code).
// Runs before `dev` and `build`. Generated files are committed so the site also builds without
// the Rust toolchain; the MCP tool list is refreshed when a built `tumr.exe` is present.
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const site = resolve(here, "..");
const repo = resolve(site, "..", "..");
const out = join(site, "content", "generated");
mkdirSync(out, { recursive: true });

const files = [
  ["docs/user/getting-started.md", "getting-started.md"],
  ["docs/user/agents.md", "agents.md"],
  ["docs/user/vscode.md", "vscode.md"],
  ["docs/user/uninstall-and-data.md", "uninstall-and-data.md"],
  ["docs/user/privacy-and-security.md", "privacy-and-security.md"],
  ["docs/user/cli-reference.md", "cli-reference.md"],
  ["docs/user/troubleshooting.md", "troubleshooting.md"],
  ["docs/agents/clients.md", "agent-clients.md"],
  ["SECURITY.md", "SECURITY.md"],
  ["CONTRIBUTING.md", "CONTRIBUTING.md"],
  ["CODE_OF_CONDUCT.md", "CODE_OF_CONDUCT.md"],
  ["THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"],
];

// screenshots used by the user guides (docs/user/images) are served from /docs-images
const imgFrom = join(repo, "docs", "user", "images");
const imgTo = join(site, "public", "docs-images");
if (existsSync(imgFrom)) {
  mkdirSync(imgTo, { recursive: true });
  for (const f of readdirSync(imgFrom)) copyFileSync(join(imgFrom, f), join(imgTo, f));
}

let copied = 0;
for (const [from, to] of files) {
  const src = join(repo, from);
  if (existsSync(src)) {
    const text = readFileSync(src, "utf8").replace(/\r\n/g, "\n");
    writeFileSync(join(out, to), text);
    copied++;
  } else if (!existsSync(join(out, to))) {
    console.warn(`sync-content: ${from} missing and no earlier copy`);
  }
}

// MCP tool catalogue straight from the product (`tumr mcp --list-tools`)
const tumr = ["target/release/tumr.exe", "target/debug/tumr.exe"].map((p) => join(repo, p)).find(existsSync);
if (tumr) {
  try {
    const json = execFileSync(tumr, ["mcp", "--list-tools"], { encoding: "utf8" });
    JSON.parse(json);
    writeFileSync(join(out, "mcp-tools.json"), json.trim() + "\n");
    copied++;
  } catch (e) {
    console.warn(`sync-content: could not list MCP tools (${e.message}); keeping the committed copy`);
  }
}

// the release manifest the release pipeline publishes (DIST-002); none published yet
const manifest = join(site, "content", "releases.json");
if (!existsSync(manifest)) {
  writeFileSync(manifest, JSON.stringify({ schema: 1, product: "Toolchain Universal Mobile Runtime", releases: [] }, null, 2) + "\n");
}
copyFileSync(manifest, join(out, "releases.json"));

console.log(`sync-content: ${copied} files refreshed into content/generated`);
