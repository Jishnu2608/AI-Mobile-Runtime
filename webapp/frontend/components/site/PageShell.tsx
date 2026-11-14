import type { ReactNode } from "react";

/** Header and reading column for the trust pages (security, privacy, licences, open source, releases). */
export function PageShell({ eyebrow, title, lead, children, aside }: { eyebrow: string; title: string; lead: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="wrap pb-24 pt-14 md:pt-20">
      <header className="max-w-3xl">
        <p className="eyebrow">{eyebrow}</p>
        <h1 className="t-page mt-5">{title}</h1>
        <p className="t-lead mt-5">{lead}</p>
      </header>
      <div className={aside ? "mt-14 grid gap-12 lg:grid-cols-12" : "mt-14"}>
        <div className={aside ? "min-w-0 lg:col-span-8" : "min-w-0"}>{children}</div>
        {aside && <aside className="lg:col-span-4">{aside}</aside>}
      </div>
    </div>
  );
}

export function DocSection({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={`${id}-title`} id={id} className="scroll-mt-28 border-t border-line pt-10 first:border-t-0 first:pt-0 [&+&]:mt-14">
      <h2 id={`${id}-title`} className="t-section">
        {title}
      </h2>
      <div className="prose mt-6 max-w-[var(--reading-max)]">{children}</div>
    </section>
  );
}
