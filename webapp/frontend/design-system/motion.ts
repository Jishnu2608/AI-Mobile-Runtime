"use client";

/**
 * Motion conventions (directive §5). GSAP runs only in client components through useGSAP,
 * scoped to a ref, so every tween and ScrollTrigger is reverted on unmount.
 *
 * - Content is in the HTML and visible without script; animations start from a state GSAP sets.
 * - Durations: fast 0.18 s (hover / press), base 0.45 s (reveals), slow 0.8 s (hero, diagrams).
 * - Eases: "power3.out" for entrances, "power2.inOut" for state changes, "none" for scrubbed lines.
 * - Under prefers-reduced-motion nothing moves: callers get `reduced === true` and set end states.
 */
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { useGSAP } from "@gsap/react";

let registered = false;
export function registerGsap() {
  if (!registered && typeof window !== "undefined") {
    gsap.registerPlugin(useGSAP, ScrollTrigger);
    registered = true;
  }
  return { gsap, ScrollTrigger };
}

export const dur = { fast: 0.18, base: 0.45, slow: 0.8 } as const;
export const ease = { enter: "power3.out", change: "power2.inOut", line: "none" } as const;

export function prefersReducedMotion() {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export { gsap, ScrollTrigger, useGSAP };
