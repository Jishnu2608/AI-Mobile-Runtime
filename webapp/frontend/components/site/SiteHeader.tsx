"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { ArrowRight, Menu, X } from "lucide-react";
import { nav } from "@/lib/site";
import { Wordmark } from "./Wordmark";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { SearchButton } from "./Search";
import { cn } from "@/lib/cn";

function isActive(path: string, href: string) {
  if (href === "/docs/getting-started/") return path.startsWith("/docs/") && !path.startsWith("/docs/requirements");
  return path.startsWith(href);
}

export function SiteHeader({ hasRelease }: { hasRelease: boolean }) {
  const path = usePathname() ?? "/";
  const cta = hasRelease ? "Download for Windows" : "See downloads";
  return (
    <header className="sticky top-0 z-50 border-b border-line bg-bg/95 backdrop-blur-md supports-[backdrop-filter]:bg-bg/90">
      <div className="wrap flex h-[var(--header-h)] items-center gap-6">
        <Link href="/" className="shrink-0 rounded-sm" aria-label="Home">
          <Wordmark />
        </Link>

        <nav aria-label="Main" className="hidden lg:block">
          <ul className="flex items-center gap-1">
            {nav.map((n) => {
              const active = isActive(path, n.href);
              return (
                <li key={n.href}>
                  <Link
                    href={n.href}
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "relative inline-flex h-11 items-center px-3 text-[0.9375rem] text-ink-2 transition-colors hover:text-ink",
                      active && "text-ink after:absolute after:inset-x-3 after:-bottom-[11px] after:h-[2px] after:rounded-full after:bg-highlight",
                    )}
                  >
                    {n.label}
                  </Link>
                </li>
              );
            })}
          </ul>
        </nav>

        <div className="ml-auto flex items-center gap-1">
          <SearchButton />
          <ThemeToggle />
          <Link href="/download/" className="btn btn-primary ml-2 hidden sm:inline-flex">
            {cta}
            <ArrowRight aria-hidden className="btn-icon size-4" />
          </Link>

          {/* phone menu: <details> works without script */}
          <details className="group lg:hidden">
            <summary
              className="flex size-11 cursor-pointer list-none items-center justify-center rounded-sm text-ink hover:bg-sunken [&::-webkit-details-marker]:hidden"
              aria-label="Menu"
            >
              <Menu aria-hidden className="size-5 group-open:hidden" />
              <X aria-hidden className="hidden size-5 group-open:block" />
            </summary>
            <div className="absolute inset-x-0 top-full z-40 max-h-[calc(100dvh-var(--header-h))] overflow-y-auto border-b border-line bg-bg shadow-2">
              <nav aria-label="Menu" className="wrap py-4">
                <ul className="divide-y divide-line">
                  {nav.map((n) => (
                    <li key={n.href}>
                      <Link
                        href={n.href}
                        aria-current={isActive(path, n.href) ? "page" : undefined}
                        className="flex min-h-12 items-center justify-between text-lg font-medium text-ink aria-[current=page]:text-link"
                        onClick={(e) => (e.currentTarget.closest("details") as HTMLDetailsElement | null)?.removeAttribute("open")}
                      >
                        {n.label}
                        <ArrowRight aria-hidden className="size-4 text-ink-2" />
                      </Link>
                    </li>
                  ))}
                </ul>
                <Link href="/download/" className="btn btn-primary mt-4 w-full">
                  {cta}
                </Link>
              </nav>
            </div>
          </details>
        </div>
      </div>
    </header>
  );
}
