import { site } from "@/lib/site";

/**
 * Text wordmark only: the brand and logo are not decided (W1 / DEC-037). The small square is a
 * neutral marker in the design system's colours, not a logo.
 */
export function Wordmark() {
  return (
    <span className="inline-flex items-center gap-2.5 font-semibold tracking-[-0.02em] text-ink">
      <span aria-hidden className="relative inline-block size-[1.15rem] rounded-[5px] bg-forest dark:bg-mint">
        <span className="absolute right-[3px] top-[3px] size-[5px] rounded-[1.5px] bg-lime" />
      </span>
      <span>{site.name}</span>
    </span>
  );
}
