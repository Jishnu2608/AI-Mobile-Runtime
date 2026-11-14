import { useId } from "react";
import { site } from "@/lib/site";

/**
 * The product mark: a phone running a coding agent's prompt (`brand/logo-mark.svg`, owner
 * request 2026-11-14). Inline, so it needs no extra request and keeps its colours in both
 * themes; the same artwork is the desktop app's icon and, in its bold small variant, the favicon.
 */
export function LogoMark({ className = "size-7" }: { className?: string }) {
  const id = useId().replace(/:/g, "");
  return (
    <svg viewBox="0 0 64 64" className={className} aria-hidden focusable="false">
      <defs>
        <linearGradient id={`umr-tile-${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2c312e" />
          <stop offset="1" stopColor="#1a1d1b" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="15" fill={`url(#umr-tile-${id})`} />
      <rect x="0.75" y="0.75" width="62.5" height="62.5" rx="14.25" fill="none" stroke="#ffffff" strokeOpacity="0.09" strokeWidth="1.5" />
      <rect x="17.5" y="8.5" width="29" height="47" rx="7.5" fill="#202321" stroke="#f0f1ed" strokeWidth="4" />
      <path d="M25 25.5 32 32l-7 6.5" fill="none" stroke="#b9e769" strokeWidth="4.5" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M34.5 38.5h6" fill="none" stroke="#b9e769" strokeWidth="4.5" strokeLinecap="round" />
    </svg>
  );
}

/** Mark + the working name (the public name is still open decision W1). */
export function Wordmark() {
  return (
    <span className="inline-flex items-center gap-2.5 font-semibold tracking-[-0.02em] text-ink">
      <LogoMark className="size-7" />
      <span>{site.name}</span>
    </span>
  );
}
