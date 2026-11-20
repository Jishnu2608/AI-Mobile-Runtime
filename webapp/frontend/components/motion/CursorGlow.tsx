"use client";

import { useEffect } from "react";

/**
 * DEC-052 enhancement (owner, 2026-11-19): a faint glow that follows the mouse across the page.
 * It only sets two CSS variables (`--cursor-x`, `--cursor-y`) and a `data-cursor` flag on <html>;
 * `body::after` in globals.css draws it. One update per frame at most, nothing stored or sent.
 * Off for touch screens (no hover), reduced motion and forced colours.
 */
export function CursorGlow() {
  useEffect(() => {
    const fine = window.matchMedia("(hover: hover) and (pointer: fine)");
    const calm = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (!fine.matches || calm.matches) return;
    const root = document.documentElement;
    let frame = 0;
    let x = 0;
    let y = 0;
    const draw = () => {
      frame = 0;
      root.style.setProperty("--cursor-x", `${x}px`);
      root.style.setProperty("--cursor-y", `${y}px`);
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      x = e.clientX;
      y = e.clientY;
      root.dataset.cursor = "on";
      if (!frame) frame = requestAnimationFrame(draw);
    };
    const leave = () => {
      delete root.dataset.cursor;
    };
    window.addEventListener("pointermove", move, { passive: true });
    document.documentElement.addEventListener("pointerleave", leave);
    window.addEventListener("blur", leave);
    return () => {
      window.removeEventListener("pointermove", move);
      document.documentElement.removeEventListener("pointerleave", leave);
      window.removeEventListener("blur", leave);
      if (frame) cancelAnimationFrame(frame);
      delete root.dataset.cursor;
    };
  }, []);
  return null;
}
