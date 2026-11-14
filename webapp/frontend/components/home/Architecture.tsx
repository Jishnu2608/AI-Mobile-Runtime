"use client";

import { useRef } from "react";
import { gsap, registerGsap, useGSAP, prefersReducedMotion, ease } from "@/design-system/motion";

const clients = [
  { name: "umr", note: "command line" },
  { name: "Desktop app", note: "wizard and preview" },
  { name: "umr mcp", note: "MCP server for agents" },
];
const modules = [
  { name: "Environment and installer", note: "scan, plan, verified downloads" },
  { name: "Devices and sessions", note: "own adb server, emulator, snapshots" },
  { name: "Framework adapters", note: "Gradle, Flutter, React Native / Expo" },
  { name: "Diagnostics", note: "six categories, dedupe, sanitiser" },
  { name: "Preview stream", note: "emulator frames to the window" },
  { name: "Agent permissions", note: "scopes, approvals, audit log" },
];
const tools = ["JDK", "Android SDK and emulator", "Flutter SDK", "Node.js and Metro", "Gradle (project wrapper)"];

/**
 * Architecture as layers (clients → user-only pipe → Runtime Core → official tools).
 * Revealed once when it scrolls into view; fully visible without script or with reduced motion.
 */
export function Architecture() {
  const root = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const { ScrollTrigger } = registerGsap();
      if (prefersReducedMotion()) return;
      const tl = gsap.timeline({
        defaults: { ease: ease.enter },
        scrollTrigger: { trigger: root.current, start: "top 75%", once: true },
      });
      tl.from("[data-arch='client']", { opacity: 0, y: 16, duration: 0.5, stagger: 0.08 })
        .from("[data-arch='pipe']", { scaleX: 0, transformOrigin: "0% 50%", duration: 0.6 }, "-=0.2")
        .from("[data-arch='module']", { opacity: 0, y: 12, duration: 0.45, stagger: 0.06 }, "-=0.3")
        .from("[data-arch='tool']", { opacity: 0, x: -8, duration: 0.4, stagger: 0.05 }, "-=0.2");
      return () => ScrollTrigger.refresh();
    },
    { scope: root },
  );

  return (
    <div ref={root} className="grid gap-4">
      <ol className="grid gap-3 sm:grid-cols-3" aria-label="Clients">
        {clients.map((c) => (
          <li key={c.name} data-arch="client" className="rounded-md border border-line bg-surface px-5 py-4">
            <p className="font-mono text-[0.95rem] font-semibold text-ink">{c.name}</p>
            <p className="t-desc mt-1">{c.note}</p>
          </li>
        ))}
      </ol>

      <div className="flex items-center gap-4 py-1" aria-hidden>
        <span data-arch="pipe" className="h-px flex-1 bg-[repeating-linear-gradient(90deg,var(--accent)_0_6px,transparent_6px_11px)]" />
        <span className="t-meta">named pipe · your Windows user only · token per client</span>
        <span data-arch="pipe" className="h-px flex-1 bg-[repeating-linear-gradient(90deg,var(--accent)_0_6px,transparent_6px_11px)]" />
      </div>

      <div className="rounded-lg bg-band p-5 text-band-ink sm:p-7 dark:bg-band dark:ring-1 dark:ring-band-line">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <p className="text-lg font-semibold tracking-tight">Runtime Core</p>
          <p className="font-mono text-xs text-band-ink-2">umr-core · one per Windows user · keeps sessions alive</p>
        </div>
        <ul className="mt-5 grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
          {modules.map((m) => (
            <li key={m.name} data-arch="module" className="rounded-md border border-band-line bg-band-surface px-4 py-3 dark:bg-band-sunken">
              <p className="font-medium">{m.name}</p>
              <p className="mt-0.5 text-sm text-band-ink-2">{m.note}</p>
            </li>
          ))}
        </ul>
      </div>

      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 pt-2">
        <span className="t-label mr-1">Official tools it runs:</span>
        {tools.map((t) => (
          <span key={t} data-arch="tool" className="badge">
            {t}
          </span>
        ))}
      </div>
    </div>
  );
}
