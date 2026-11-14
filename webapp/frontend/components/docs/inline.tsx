import type { ReactNode } from "react";

/** Renders `code` spans inside a plain string prop (MDX attributes are not markdown). */
export function inline(text: string): ReactNode {
  return text.split(/(`[^`]+`)/g).map((part, i) =>
    part.startsWith("`") && part.endsWith("`") ? (
      <code key={i} className="rounded-xs border border-line bg-[var(--code-bg)] px-1 py-px font-mono text-[0.86em]">
        {part.slice(1, -1)}
      </code>
    ) : (
      part
    ),
  );
}
