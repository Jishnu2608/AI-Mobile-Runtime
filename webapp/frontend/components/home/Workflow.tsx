"use client";

import { useRef, useState, type KeyboardEvent } from "react";
import { Bot, User } from "lucide-react";
import { gsap, registerGsap, useGSAP, prefersReducedMotion, ease } from "@/design-system/motion";
import { stages, type Part } from "./workflow-data";
import { cn } from "@/lib/cn";

/** Positions of the parts in the schematic (viewBox 360 × 300). */
const parts: Record<Part, { x: number; y: number; w: number; label: string }> = {
  project: { x: 16, y: 20, w: 120, label: "Project folder" },
  toolchain: { x: 16, y: 112, w: 120, label: "Toolchain" },
  agent: { x: 16, y: 210, w: 120, label: "Coding agent" },
  core: { x: 168, y: 112, w: 120, label: "Runtime Core" },
  emulator: { x: 232, y: 20, w: 112, label: "Emulator" },
  preview: { x: 232, y: 210, w: 112, label: "Preview" },
  problems: { x: 168, y: 260, w: 120, label: "Problems" },
};
const links: [Part, Part][] = [
  ["project", "core"],
  ["toolchain", "core"],
  ["agent", "core"],
  ["core", "emulator"],
  ["core", "preview"],
  ["core", "problems"],
];
const centre = (p: Part) => ({ x: parts[p].x + parts[p].w / 2, y: parts[p].y + 15 });

/**
 * The eight-stage loop as tabs (directive §5.5): the reader chooses the stage — nothing advances
 * on a timer and nothing is tied to scrolling. Arrow keys move between stages.
 */
export function Workflow() {
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const stage = stages[active];

  const { contextSafe } = useGSAP({ scope: root });

  const select = contextSafe((i: number) => {
    registerGsap();
    setActive(i);
    if (!prefersReducedMotion()) {
      gsap.fromTo("[data-stage-panel]", { opacity: 0, y: 8 }, { opacity: 1, y: 0, duration: 0.35, ease: ease.enter });
      gsap.fromTo("[data-part-on]", { scale: 0.96 }, { scale: 1, duration: 0.4, ease: "back.out(2)", transformOrigin: "50% 50%" });
    }
  });

  function onKey(e: KeyboardEvent<HTMLButtonElement>, i: number) {
    const last = stages.length - 1;
    const next = e.key === "ArrowDown" || e.key === "ArrowRight" ? (i === last ? 0 : i + 1) : e.key === "ArrowUp" || e.key === "ArrowLeft" ? (i === 0 ? last : i - 1) : e.key === "Home" ? 0 : e.key === "End" ? last : -1;
    if (next >= 0) {
      e.preventDefault();
      select(next);
      tabs.current[next]?.focus();
    }
  }

  const on = new Set(stage.parts);
  return (
    <div ref={root} className="grid gap-8 lg:grid-cols-12 lg:gap-12">
      <div role="tablist" aria-label="Workflow stages" aria-orientation="vertical" className="flex flex-col lg:col-span-5">
        {stages.map((s, i) => {
          const selected = i === active;
          return (
            <button
              key={s.id}
              ref={(el) => {
                tabs.current[i] = el;
              }}
              role="tab"
              id={`wf-tab-${s.id}`}
              aria-selected={selected}
              aria-controls="wf-panel"
              tabIndex={selected ? 0 : -1}
              onClick={() => select(i)}
              onKeyDown={(e) => onKey(e, i)}
              className={cn(
                "group relative flex min-h-12 items-center gap-4 border-b border-line py-3 pl-1 pr-3 text-left transition-colors",
                selected ? "text-ink" : "text-ink-2 hover:text-ink",
              )}
            >
              <span
                aria-hidden
                className={cn(
                  "font-mono text-sm tabular-nums transition-colors",
                  selected ? "text-highlight" : "text-ink-2 group-hover:text-ink",
                )}
              >
                {String(i + 1).padStart(2, "0")}
              </span>
              <span className="flex-1 text-[1.0625rem] font-medium tracking-tight">{s.title}</span>
              <span
                aria-hidden
                className={cn("h-[2px] origin-left bg-highlight transition-all duration-300", selected ? "w-8" : "w-0 group-hover:w-3")}
              />
            </button>
          );
        })}
      </div>

      <div
        id="wf-panel"
        role="tabpanel"
        aria-labelledby={`wf-tab-${stage.id}`}
        tabIndex={0}
        className="rounded-lg border border-line bg-band-sunken p-5 sm:p-7 lg:col-span-7"
      >
        <div data-stage-panel>
          <div className="flex flex-wrap items-center gap-3">
            <span className="badge border-band-line bg-transparent text-band-ink-2">
              {stage.who === "you" ? <User aria-hidden className="size-3.5" /> : <Bot aria-hidden className="size-3.5" />}
              {stage.who === "you" ? "You run" : "Your agent calls"}
            </span>
            <span className="t-meta text-band-ink-2">
              Stage {active + 1} of {stages.length}
            </span>
          </div>
          <h3 className="t-sub mt-4 text-band-ink">{stage.title}</h3>
          <p className="mt-3 max-w-xl leading-relaxed text-band-ink-2">{stage.body}</p>
          <pre className="mt-5 overflow-x-auto rounded-md border border-band-line bg-band px-4 py-3 font-mono text-sm leading-relaxed text-mint">
            {stage.command}
          </pre>
        </div>

        <svg viewBox="0 0 360 300" className="mx-auto mt-6 h-auto w-full max-w-[26rem]" role="img" aria-label={`Parts involved: ${stage.parts.map((p) => parts[p].label).join(", ")}`}>
          {links.map(([a, b]) => {
            const A = centre(a);
            const B = centre(b);
            const lit = on.has(a) && on.has(b);
            return (
              <line
                key={`${a}-${b}`}
                x1={A.x}
                y1={A.y}
                x2={B.x}
                y2={B.y}
                stroke={lit ? "var(--lime)" : "var(--band-line)"}
                strokeWidth={lit ? 2 : 1}
                strokeDasharray={lit ? undefined : "3 4"}
                style={{ transition: "stroke 300ms" }}
              />
            );
          })}
          {(Object.keys(parts) as Part[]).map((k) => {
            const p = parts[k];
            const lit = on.has(k);
            return (
              <g key={k} {...(lit ? { "data-part-on": "" } : {})}>
                <rect
                  x={p.x}
                  y={p.y}
                  width={p.w}
                  height={30}
                  rx={6}
                  fill={lit ? (k === "core" ? "var(--lime)" : "var(--band-surface)") : "var(--band-sunken)"}
                  stroke={lit ? "var(--lime)" : "var(--band-line)"}
                  style={{ transition: "fill 300ms, stroke 300ms" }}
                />
                <text
                  x={p.x + p.w / 2}
                  y={p.y + 19.5}
                  textAnchor="middle"
                  fontSize="11.5"
                  fontWeight={lit ? 600 : 400}
                  fill={lit ? (k === "core" ? "var(--forest)" : "var(--band-ink)") : "var(--band-ink-3)"}
                >
                  {p.label}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
