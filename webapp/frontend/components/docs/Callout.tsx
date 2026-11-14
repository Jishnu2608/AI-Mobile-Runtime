import { AlertTriangle, Info, Lightbulb, XOctagon } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

const styles = {
  note: { icon: Info, cls: "border-l-accent bg-sunken", label: "Note" },
  tip: { icon: Lightbulb, cls: "border-l-[var(--lime-deep)] bg-lime-pale/50 dark:bg-accent-soft", label: "Tip" },
  warning: { icon: AlertTriangle, cls: "border-l-warn bg-warn-bg", label: "Warning" },
  danger: { icon: XOctagon, cls: "border-l-err bg-err-bg", label: "Important" },
} as const;

export function Callout({ type = "note", title, children }: { type?: keyof typeof styles; title?: string; children: ReactNode }) {
  const s = styles[type];
  const Icon = s.icon;
  return (
    <aside className={cn("not-prose my-6 rounded-r-md border border-line border-l-[3px] px-5 py-4", s.cls)}>
      <p className="mb-1 flex items-center gap-2 text-sm font-semibold text-ink">
        <Icon aria-hidden className="size-4" />
        {title ?? s.label}
      </p>
      <div className="text-[0.95rem] leading-relaxed text-ink [&>p+p]:mt-2 [&_a]:text-link [&_a]:underline [&_code]:font-mono [&_code]:text-[0.86em]">{children}</div>
    </aside>
  );
}
