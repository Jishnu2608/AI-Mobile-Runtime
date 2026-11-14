import type { ReactNode } from "react";
import GithubSlugger from "github-slugger";
import { Status, type StatusKind } from "@/components/ui/Status";
import { inline } from "./inline";

const categories: Record<string, { label: string; kind: StatusKind }> = {
  build: { label: "Build · your code", kind: "err" },
  runtime: { label: "Runtime · your code", kind: "warn" },
  crash: { label: "Crash", kind: "err" },
  environment: { label: "Environment", kind: "warn" },
  device: { label: "Device", kind: "warn" },
  network: { label: "Network", kind: "info" },
};

/**
 * One troubleshooting entry: symptoms, likely cause, how to check, resolution (directive §7.5).
 * The category matches what `umr problems` prints, so a reader can find their problem by it.
 */
export function Issue({
  title,
  category,
  symptoms,
  cause,
  check,
  children,
}: {
  title: string;
  category: keyof typeof categories;
  symptoms: string;
  cause: string;
  check: string;
  children: ReactNode;
}) {
  const c = categories[category];
  const id = new GithubSlugger().slug(title);
  return (
    <section id={id} className="not-prose my-6 scroll-mt-24 overflow-hidden rounded-md border border-line bg-surface">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-line bg-sunken px-5 py-3.5">
        <h3 className="text-[1.05rem] font-semibold tracking-tight text-ink">
          <a href={`#${id}`} className="hover:underline">{title}</a>
        </h3>
        <Status kind={c.kind}>{c.label}</Status>
      </header>
      <dl className="grid gap-x-6 gap-y-3 px-5 py-4 text-[0.95rem] leading-relaxed sm:grid-cols-[9rem_1fr]">
        <dt className="font-medium text-ink-2">Symptoms</dt>
        <dd className="text-ink">{inline(symptoms)}</dd>
        <dt className="font-medium text-ink-2">Likely cause</dt>
        <dd className="text-ink">{inline(cause)}</dd>
        <dt className="font-medium text-ink-2">How to check</dt>
        <dd className="text-ink">{inline(check)}</dd>
        <dt className="font-medium text-ink-2">Resolution</dt>
        <dd className="text-ink [&_a]:text-link [&_a]:underline [&_code]:font-mono [&_code]:text-[0.86em] [&_p+p]:mt-2">{children}</dd>
      </dl>
    </section>
  );
}
