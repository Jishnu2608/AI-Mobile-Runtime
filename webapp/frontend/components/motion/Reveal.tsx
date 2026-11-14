"use client";

import { useRef, type ReactNode } from "react";
import { gsap, registerGsap, useGSAP, prefersReducedMotion, ease } from "@/design-system/motion";

/**
 * Reveals `[data-reveal]` children once, as they enter the viewport (ScrollTrigger.batch, once).
 * No scrub, no pin: scrolling stays the reader's. Content is visible without script, and with
 * reduced motion nothing moves. useGSAP reverts every tween and trigger on unmount.
 */
export function Reveal({ children, className }: { children: ReactNode; className?: string }) {
  const root = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const { ScrollTrigger } = registerGsap();
      if (prefersReducedMotion() || !root.current) return;
      const items = gsap.utils.toArray<HTMLElement>("[data-reveal]", root.current);
      if (items.length === 0) return;
      gsap.set(items, { opacity: 0, y: 14 });
      ScrollTrigger.batch(items, {
        start: "top 88%",
        once: true,
        onEnter: (batch) => gsap.to(batch, { opacity: 1, y: 0, duration: 0.55, ease: ease.enter, stagger: 0.07, overwrite: true }),
      });
    },
    { scope: root },
  );

  return (
    <div ref={root} className={className}>
      {children}
    </div>
  );
}
