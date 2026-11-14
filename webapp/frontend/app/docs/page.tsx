import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight } from "lucide-react";
import { docGroups, docHref, docs } from "@/lib/docs";

export const metadata: Metadata = {
  title: "Documentation",
  description: "Install the runtime, run a Kotlin, Java, Flutter, React Native or Expo app, connect a coding agent, and fix common problems.",
};

export default function DocsIndex() {
  return (
    <div>
      <header className="max-w-[var(--reading-max)]">
        <p className="eyebrow">Documentation</p>
        <h1 className="t-page mt-5">Set up, run and connect</h1>
        <p className="t-lead mt-5">
          Start with the getting-started guide. It takes you from the installer to a running app and a connected agent.
        </p>
        <Link href="/docs/getting-started/" className="btn btn-primary mt-8 h-11 px-5">
          Read the getting-started guide
          <ArrowRight aria-hidden className="btn-icon size-4" />
        </Link>
      </header>
      <div className="mt-14 grid gap-12">
        {docGroups.map((g) => (
          <section key={g} aria-labelledby={`g-${g}`}>
            <h2 id={`g-${g}`} className="t-label">{g}</h2>
            <ul className="mt-4 grid gap-3 sm:grid-cols-2 2xl:grid-cols-3">
              {docs
                .filter((d) => d.group === g)
                .map((d) => (
                  <li key={d.title}>
                    <Link href={docHref(d)} className="card-link flex h-full flex-col rounded-md border border-line bg-surface p-5">
                      <span className="font-semibold text-ink">{d.title}</span>
                      <span className="t-desc mt-1.5">{d.description}</span>
                    </Link>
                  </li>
                ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
