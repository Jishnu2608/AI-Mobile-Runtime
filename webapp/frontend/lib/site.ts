/**
 * Site facts and the owner decisions still open (todo.md → Open Decisions — Phase 9).
 * A `null` here is a pending decision: pages show a labelled placeholder, never a guess.
 */
export const site = {
  /** W1 / DEC-037 (owner, 2026-11-20). */
  name: "Toolchain Universal Mobile Runtime",
  shortName: "T-UMR",
  /** The command line tool. */
  command: "tumr",
  /** No domain yet: no absolute URLs, sitemap or canonical links. */
  domain: null as string | null,
  /** The product licence (SPDX id). */
  licence: "Apache-2.0",
  /** Where the website is hosted (release files are on GitHub Releases). */
  hosting: null as string | null,
  releasesHost: "GitHub Releases",
  repositoryPublic: true,
  repository: "https://github.com/Jishnu2608/TUMR",
  platform: "Windows 11 (x64)",
  frameworks: ["Kotlin", "Java", "Flutter", "React Native", "Expo"] as const,
  /** DEC-054 (owner, 2026-11-19): every client `tumr agent setup` writes is supported in v1. */
  agents: ["Claude Code", "Codex", "Cursor", "Antigravity", "Claude Desktop", "VS Code"] as const,
  /** The VS Code extension: set `published: true` once each store listing is live. */
  vscode: {
    id: "tumr.tumr-vscode",
    vsix: "tumr-vscode.vsix",
    marketplace: {
      url: "https://marketplace.visualstudio.com/items?itemName=tumr.tumr-vscode",
      published: false as boolean,
    },
    openVsx: {
      url: "https://open-vsx.org/extension/tumr/tumr-vscode",
      published: false as boolean,
    },
  },
} as const;

export const nav = [
  { href: "/docs/getting-started/", label: "Docs" },
  { href: "/docs/requirements/", label: "Requirements" },
  { href: "/releases/", label: "Releases" },
  { href: "/security/", label: "Security" },
  { href: "/open-source/", label: "Open source" },
] as const;
