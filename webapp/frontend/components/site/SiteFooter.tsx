import Link from "next/link";
import { Wordmark } from "./Wordmark";
import { docs, docHref } from "@/lib/docs";

const columns = [
  {
    title: "Product",
    links: [
      { href: "/", label: "Overview" },
      { href: "/download/", label: "Downloads" },
      { href: "/releases/", label: "Release notes" },
      { href: "/docs/requirements/", label: "Requirements" },
    ],
  },
  {
    title: "Docs",
    links: docs
      .filter((d) => ["getting-started", "concepts", "agents", "cli", "mcp", "troubleshooting"].includes(d.slug[d.slug.length - 1]))
      .map((d) => ({ href: docHref(d), label: d.title })),
  },
  {
    title: "Trust",
    links: [
      { href: "/security/", label: "Security" },
      { href: "/privacy/", label: "Privacy" },
      { href: "/licences/", label: "Licences" },
      { href: "/open-source/", label: "Open source" },
    ],
  },
];

export function SiteFooter() {
  return (
    <footer className="mt-auto border-t border-line bg-bg-alt">
      <div className="wrap grid gap-10 py-14 md:grid-cols-12">
        <div className="md:col-span-5">
          <Wordmark />
          <p className="t-desc mt-4 max-w-sm">
            A local runtime for Windows 11 that shows your Android app beside your coding agent. It runs on your PC and
            downloads only the tools you approve.
          </p>
        </div>
        {columns.map((c, i) => (
          <nav key={c.title} aria-label={c.title} className={i === 0 ? "md:col-span-2 md:col-start-7" : "md:col-span-2"}>
            <p className="t-label mb-3 text-ink">{c.title}</p>
            <ul className="space-y-1">
              {c.links.map((l) => (
                <li key={l.href}>
                  <Link href={l.href} className="draw inline-flex min-h-9 items-center text-[0.9375rem] text-ink-2 hover:text-ink">
                    {l.label}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="border-t border-line">
        <p className="wrap py-5 text-[0.8125rem] text-ink-2">
          Working name, pending a decision. Android is a trademark of Google LLC; this project is not affiliated with Google.
          Product names are trademarks of their owners.
        </p>
      </div>
    </footer>
  );
}
