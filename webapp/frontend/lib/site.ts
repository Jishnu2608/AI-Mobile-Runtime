/**
 * Site facts and the owner decisions still open (todo.md → Open Decisions — Phase 9).
 * A `null` here is a pending decision: pages show a labelled placeholder, never a guess.
 */
export const site = {
  name: "AI Mobile Runtime",
  /** No domain yet: no absolute URLs, sitemap or canonical links. */
  domain: null as string | null,
  /** The product licence, once published. */
  licence: null as string | null,
  /** Where the website is hosted (release files are on GitHub Releases). */
  hosting: null as string | null,
  releasesHost: "GitHub Releases",
  repositoryPublic: true,
  repository: "https://github.com/Jishnu2608/AI-Mobile-Runtime",
  platform: "Windows 11 (x64)",
  frameworks: ["Kotlin", "Java", "Flutter", "React Native", "Expo"] as const,
  agents: ["Claude Code", "Codex"] as const,
  /** The VS Code extension: set `published: true` once each store listing is live. */
  vscode: {
    id: "ai-mobile-runtime.umr-vscode",
    vsix: "umr-vscode.vsix",
    marketplace: {
      url: "https://marketplace.visualstudio.com/items?itemName=ai-mobile-runtime.umr-vscode",
      published: false as boolean,
    },
    openVsx: {
      url: "https://open-vsx.org/extension/ai-mobile-runtime/umr-vscode",
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
