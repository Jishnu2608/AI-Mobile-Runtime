import Link from "next/link";

/**
 * Site-wide notice while owner decisions are open (todo.md → Open Decisions — Phase 9), so no
 * placeholder is mistaken for a final decision.
 */
export function PendingBanner() {
  return (
    <div className="border-b border-line bg-bg-alt text-[0.8125rem] leading-snug text-ink-2">
      <p className="wrap flex flex-wrap items-center gap-x-2 gap-y-0.5 py-2">
        <span className="badge badge-lime">Preview site</span>
        <span>
          “AI Mobile Runtime” is a working name. The public name, domain, licence and hosting are not decided yet, and no
          release is published.
        </span>
        <Link href="/open-source/#open-decisions" className="link font-medium">
          See open decisions
        </Link>
      </p>
    </div>
  );
}
