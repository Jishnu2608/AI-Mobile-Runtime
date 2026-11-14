"use client";

import { useSyncExternalStore } from "react";
import { Moon, Sun } from "lucide-react";

type Theme = "light" | "dark";

function subscribe(onChange: () => void) {
  const o = new MutationObserver(onChange);
  o.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => o.disconnect();
}
function readTheme(): Theme {
  return document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light";
}

/** Explicit light/dark switch; the choice is kept in localStorage (no cookie). */
export function ThemeToggle() {
  const theme = useSyncExternalStore(subscribe, readTheme, () => null);

  function toggle() {
    const next: Theme = theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    try {
      localStorage.setItem("theme", next);
    } catch {
      /* private mode: the choice lasts for this page only */
    }
  }

  const label = theme === "dark" ? "Switch to light theme" : "Switch to dark theme";
  return (
    <button
      type="button"
      onClick={toggle}
      aria-label={theme ? label : "Switch theme"}
      title={theme ? label : "Switch theme"}
      className="inline-flex size-11 items-center justify-center rounded-sm text-ink-2 transition-colors hover:bg-sunken hover:text-ink"
    >
      {/* both icons in the HTML; CSS shows the right one before hydration */}
      <Sun aria-hidden className="size-[18px] hidden dark:block" />
      <Moon aria-hidden className="size-[18px] block dark:hidden" />
    </button>
  );
}
