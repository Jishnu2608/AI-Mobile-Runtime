"use client";

import { useRef } from "react";
import Link from "next/link";
import { ArrowRight, BookOpen } from "lucide-react";
import { MotionPathPlugin } from "gsap/MotionPathPlugin";
import { gsap, registerGsap, useGSAP, prefersReducedMotion, ease } from "@/design-system/motion";
import { HeroDiagram } from "./HeroDiagram";

export function Hero({ hasRelease, toolCount }: { hasRelease: boolean; toolCount: number }) {
  const root = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      registerGsap();
      gsap.registerPlugin(MotionPathPlugin);
      if (prefersReducedMotion()) return;

      const tl = gsap.timeline({ defaults: { ease: ease.enter } });
      tl.from("[data-enter]", { y: 18, opacity: 0, duration: 0.7, stagger: 0.08 })
        .from("[data-node]", { opacity: 0, y: 10, duration: 0.6, stagger: 0.09 }, 0.25);

      // draw the solid connectors
      root.current?.querySelectorAll<SVGPathElement>("[data-lines] path").forEach((p, i) => {
        if (p.getAttribute("stroke-dasharray")) {
          tl.from(p, { opacity: 0, duration: 0.5 }, 0.7 + i * 0.08);
          return;
        }
        const len = p.getTotalLength();
        gsap.set(p, { strokeDasharray: len, strokeDashoffset: len });
        tl.to(p, { strokeDashoffset: 0, duration: 0.55, ease: ease.change }, 0.7 + i * 0.08);
      });

      // pulses run along the data paths a few times, then rest (no endless motion)
      root.current?.querySelectorAll<SVGCircleElement>("[data-pulse]").forEach((c, i) => {
        const path = `#${c.dataset.pulse}`;
        tl.fromTo(
          c,
          { opacity: 0 },
          {
            opacity: 1,
            duration: 1.5,
            repeat: 2,
            repeatDelay: 0.6,
            ease: "power1.inOut",
            motionPath: { path, align: path, alignOrigin: [0.5, 0.5] },
            onComplete: () => {
              gsap.to(c, { opacity: 0, duration: 0.3 });
            },
          },
          1.4 + i * 0.5,
        );
      });
    },
    { scope: root },
  );

  return (
    <section ref={root} aria-labelledby="hero-title" className="relative overflow-hidden">
      {/* soft tonal field behind the hero, flat colour, no orb */}
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-full bg-[linear-gradient(180deg,var(--bg-alt)_0%,var(--bg)_70%)]" />
      <div className="wrap grid items-center gap-12 pt-14 pb-20 md:pt-20 lg:min-h-[calc(100dvh-var(--header-h)-2.5rem)] lg:grid-cols-12 lg:gap-10 lg:pb-24">
        <div className="lg:col-span-6 xl:col-span-6">
          <p className="eyebrow" data-enter>
            Local runtime for Windows 11
          </p>
          <h1 id="hero-title" className="t-display mt-6 max-w-[14ch]" data-enter>
            The Android runtime for <span className="relative whitespace-nowrap text-pine dark:text-jade">AI coding agents</span>
          </h1>
          <p className="t-lead mt-7 max-w-[34rem]" data-enter>
            It sets up the Android toolchain, runs the emulator, docks your running app beside Claude Code or Codex, and hands
            the agent the screen, the errors and the controls.
          </p>
          <div className="mt-9 flex flex-wrap items-center gap-3" data-enter>
            <Link href="/download/" className="btn btn-primary h-12 px-5 text-base">
              {hasRelease ? "Download for Windows" : "See downloads"}
              <ArrowRight aria-hidden className="btn-icon size-4" />
            </Link>
            <Link href="/docs/getting-started/" className="btn h-12 px-5 text-base">
              <BookOpen aria-hidden className="size-4" />
              Read the getting-started guide
            </Link>
          </div>
          <dl className="mt-10 grid max-w-[36rem] grid-cols-1 gap-px sm:grid-cols-3 overflow-hidden rounded-md border border-line bg-line text-sm" data-enter>
            {[
              ["Host", "Windows 11 x64"],
              ["Frameworks", "Kotlin, Java, Flutter, React Native, Expo"],
              ["Agent tools", `${toolCount} MCP tools`],
            ].map(([k, v]) => (
              <div key={k} className="bg-surface px-4 py-3">
                <dt className="t-label">{k}</dt>
                <dd className="mt-1 font-medium leading-snug text-ink t-num">{v}</dd>
              </div>
            ))}
          </dl>
        </div>
        <div className="lg:col-span-6 xl:col-span-6">
          <div className="rounded-lg border border-line bg-surface/60 p-3 shadow-1 sm:p-5">
            <HeroDiagram />
          </div>
        </div>
      </div>
    </section>
  );
}
