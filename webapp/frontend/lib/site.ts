/**
 * Site facts and the owner decisions still open (todo.md → Open Decisions — Phase 9).
 * A `null` here is a pending decision: pages show a labelled placeholder, never a guess.
 */
export const site = {
  /** W1 / DEC-037 pending: the name the app uses today, shown as a working name. */
  name: "AI Mobile Runtime",
  nameIsWorking: true,
  /** W2 / DEC-038 pending: no domain, so no absolute URLs, sitemap or canonical links. */
  domain: null as string | null,
  /** W3 / DEC-025 pending: no product licence yet. */
  licence: null as string | null,
  /** W5 / DEC-040 pending: hosting; the site is a static export. */
  hosting: null as string | null,
  /** The source repository is private until W3 is decided; nothing links to it. */
  repositoryPublic: false,
  platform: "Windows 11 (x64)",
  frameworks: ["Kotlin", "Java", "Flutter", "React Native", "Expo"] as const,
  agents: ["Claude Code", "Codex"] as const,
} as const;

/** The ten Phase 9 decisions, for the pending-decisions notice and the open-source page. */
export const pendingDecisions = [
  { w: "W1", id: "DEC-037", what: "Public product name and brand identity" },
  { w: "W2", id: "DEC-038", what: "Domain" },
  { w: "W3", id: "DEC-025", what: "Product licence" },
  { w: "W5", id: "DEC-040", what: "Hosting and release distribution" },
  { w: "W7", id: "DEC-027", what: "Website telemetry policy" },
  { w: "W8", id: "DEC-011 + DEC-033", what: "Release signing and key custody" },
  { w: "W9", id: "DEC-042", what: "Installer PATH option default" },
  { w: "W10", id: "DEC-043", what: "macOS / Linux visitor experience" },
] as const;

export const nav = [
  { href: "/docs/getting-started/", label: "Docs" },
  { href: "/docs/requirements/", label: "Requirements" },
  { href: "/releases/", label: "Releases" },
  { href: "/security/", label: "Security" },
  { href: "/open-source/", label: "Open source" },
] as const;
