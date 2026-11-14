import { docs, docHref, readSource, toc } from "@/lib/docs";
import { loadTools } from "@/components/docs/McpToolTable";

export const dynamic = "force-static";

function plain(md: string) {
  return md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/[#*_`>|\-[\]()]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 4000);
}

/** The static search index, built with the site (no search service, no request logging). */
export function GET() {
  const entries = docs.map((d) => {
    const src = readSource(d);
    return { href: docHref(d), title: d.title, section: `Docs · ${d.group}`, headings: toc(src).map((t) => t.text), text: `${d.description} ${plain(src)}` };
  });
  for (const t of loadTools()) {
    entries.push({ href: `/docs/reference/mcp/#${t.name}`, title: t.name, section: "MCP tool", headings: [t.title], text: t.description });
  }
  entries.push(
    { href: "/download/", title: "Downloads", section: "Download", headings: ["Installer", "Portable ZIP", "Verify the download"], text: "download installer zip sha256 checksum signature windows 11" },
    { href: "/security/", title: "Security", section: "Trust", headings: ["Reporting a vulnerability", "Review"], text: "security vulnerability report review threat model" },
    { href: "/privacy/", title: "Privacy", section: "Trust", headings: [], text: "privacy telemetry local data" },
    { href: "/releases/", title: "Release notes", section: "Releases", headings: [], text: "releases changelog versions" },
  );
  return Response.json(entries);
}
