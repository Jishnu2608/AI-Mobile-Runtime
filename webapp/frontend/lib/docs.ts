import { readFileSync } from "node:fs";
import { join } from "node:path";
import GithubSlugger from "github-slugger";

/** One documentation page: where its text comes from and where it sits in the navigation. */
export type DocPage = {
  slug: string[];
  title: string;
  description: string;
  group: "Start" | "Guides" | "Reference" | "Help";
  /** generated = a copy of a repository document (content/generated); site = written for the site. */
  source: { kind: "generated" | "site"; file: string; format: "md" | "mdx" };
};

export const docs: DocPage[] = [
  {
    slug: ["getting-started"],
    title: "Getting started",
    description: "Install, run the setup wizard, run your first app and connect your coding agent.",
    group: "Start",
    source: { kind: "generated", file: "getting-started.md", format: "md" },
  },
  {
    slug: ["requirements"],
    title: "Requirements",
    description: "What your PC needs, what the runtime installs for you, and what it never changes.",
    group: "Start",
    source: { kind: "site", file: "requirements.mdx", format: "mdx" },
  },
  {
    slug: ["concepts"],
    title: "How it works",
    description: "The Runtime Core, its clients, sessions, devices and where files go.",
    group: "Start",
    source: { kind: "site", file: "concepts.mdx", format: "mdx" },
  },
  {
    slug: ["guides", "kotlin-java"],
    title: "Kotlin and Java (Gradle)",
    description: "Run an Android Studio project through its own Gradle wrapper.",
    group: "Guides",
    source: { kind: "site", file: "guide-kotlin-java.mdx", format: "mdx" },
  },
  {
    slug: ["guides", "flutter"],
    title: "Flutter",
    description: "Hot reload, hot restart and full rebuilds on the emulator.",
    group: "Guides",
    source: { kind: "site", file: "guide-flutter.mdx", format: "mdx" },
  },
  {
    slug: ["guides", "react-native-expo"],
    title: "React Native and Expo",
    description: "Expo Go, Expo dev builds and bare React Native with Metro.",
    group: "Guides",
    source: { kind: "site", file: "guide-react-native.mdx", format: "mdx" },
  },
  {
    slug: ["guides", "agents"],
    title: "Coding agents (MCP)",
    description: "Connect Claude Code, Codex or another MCP client; tools and permissions.",
    group: "Guides",
    source: { kind: "generated", file: "agents.md", format: "md" },
  },
  {
    slug: ["guides", "device-profiles"],
    title: "Device profiles",
    description: "Phone, foldable and tablet sizes, and switching between them.",
    group: "Guides",
    source: { kind: "site", file: "guide-devices.mdx", format: "mdx" },
  },
  {
    slug: ["reference", "cli"],
    title: "umr command reference",
    description: "Every umr command and option, generated from the program's own help.",
    group: "Reference",
    source: { kind: "generated", file: "cli-reference.md", format: "md" },
  },
  {
    slug: ["reference", "mcp"],
    title: "MCP tool reference",
    description: "Every tool the umr MCP server offers, with its arguments, generated from the product.",
    group: "Reference",
    source: { kind: "site", file: "reference-mcp.mdx", format: "mdx" },
  },
  {
    slug: ["troubleshooting"],
    title: "Troubleshooting",
    description: "Problems by category: symptoms, likely cause, how to check and how to fix.",
    group: "Help",
    source: { kind: "site", file: "troubleshooting.mdx", format: "mdx" },
  },
  {
    slug: ["uninstall"],
    title: "Uninstall and your data",
    description: "What is installed where, what uninstalling removes and what it keeps.",
    group: "Help",
    source: { kind: "generated", file: "uninstall-and-data.md", format: "md" },
  },
];

export const docGroups = ["Start", "Guides", "Reference", "Help"] as const;

export function docHref(d: DocPage) {
  return `/docs/${d.slug.join("/")}/`;
}

export function findDoc(slug: string[]) {
  return docs.find((d) => d.slug.join("/") === slug.join("/"));
}

const contentRoot = join(process.cwd(), "content");

export function readSource(d: DocPage): string {
  const dir = d.source.kind === "generated" ? "generated" : "docs";
  let text = readFileSync(join(contentRoot, dir, d.source.file), "utf8");
  // the page renders its own title: drop the document's first h1
  text = text.replace(/^# .*\n+/, "");
  return text;
}

export function readGenerated(file: string): string {
  return readFileSync(join(contentRoot, "generated", file), "utf8").replace(/^# .*\n+/, "");
}

export type TocItem = { depth: 2 | 3; text: string; id: string };

/** h2 / h3 of a markdown text, with the ids rehype-slug will give them. */
export function toc(markdown: string): TocItem[] {
  const slugger = new GithubSlugger();
  const items: TocItem[] = [];
  let fenced = false;
  for (const line of markdown.split("\n")) {
    if (line.startsWith("```")) fenced = !fenced;
    if (fenced) continue;
    const m = /^(#{2,3}) (.+)$/.exec(line);
    if (m) {
      const text = m[2].replace(/`/g, "").replace(/\[(.*?)\]\(.*?\)/g, "$1").trim();
      items.push({ depth: m[1].length as 2 | 3, text, id: slugger.slug(text) });
    }
  }
  return items;
}

/** Repository-relative markdown links → site routes. */
const linkMap: Record<string, string> = {
  "README.md": "/docs/",
  "getting-started.md": "/docs/getting-started/",
  "requirements.md": "/docs/requirements/",
  "agents.md": "/docs/guides/agents/",
  "troubleshooting.md": "/docs/troubleshooting/",
  "privacy-and-security.md": "/privacy/",
  "uninstall-and-data.md": "/docs/uninstall/",
  "cli-reference.md": "/docs/reference/cli/",
  "SECURITY.md": "/security/",
  "CONTRIBUTING.md": "/open-source/#contributing",
  "CODE_OF_CONDUCT.md": "/open-source/#code-of-conduct",
  "review-v1.md": "/security/#review",
  "THIRD_PARTY_NOTICES.md": "/licences/",
};

export function rewriteHref(href: string | undefined): string | undefined {
  if (!href || /^[a-z]+:/i.test(href) || href.startsWith("#") || href.startsWith("/")) return href;
  const [path, hash] = href.split("#");
  const base = path.split("/").filter(Boolean).pop() ?? "";
  if (base === "" || base === "security") return "/security/";
  const target = linkMap[base];
  if (!target) return href;
  return hash && !target.includes("#") ? `${target}#${hash}` : target;
}
