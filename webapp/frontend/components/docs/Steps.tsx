import type { ReactNode } from "react";

/** Numbered step-by-step instructions (ordered list, so screen readers count them). */
export function Steps({ children }: { children: ReactNode }) {
  return <ol className="not-prose my-8 space-y-0 [counter-reset:step]">{children}</ol>;
}

export function Step({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="group relative grid grid-cols-[2.25rem_1fr] gap-x-4 pb-8 [counter-increment:step] last:pb-0">
      <span
        aria-hidden
        className="relative z-10 flex size-9 items-center justify-center rounded-full border border-line-strong bg-surface font-mono text-sm font-semibold text-ink before:content-[counter(step)]"
      />
      <span aria-hidden className="absolute left-[1.125rem] top-9 bottom-0 w-px bg-line group-last:hidden" />
      <div className="pt-1">
        <p className="font-semibold text-ink">{title}</p>
        <div className="mt-2 space-y-3 text-[0.98rem] leading-relaxed text-ink [&_a]:text-link [&_a]:underline [&_p:not(:first-child)]:mt-2">{children}</div>
      </div>
    </li>
  );
}
