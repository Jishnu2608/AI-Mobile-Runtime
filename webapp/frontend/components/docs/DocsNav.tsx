"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/cn";

type Item = { href: string; title: string };
type Group = { name: string; items: Item[] };

/** Left navigation of the docs. On narrow screens it folds into a <details> (works without script). */
export function DocsNav({ groups }: { groups: Group[] }) {
  const path = usePathname();
  const current = groups.flatMap((g) => g.items).find((i) => i.href === path || i.href === `${path}/`);
  const list = (
    <nav aria-label="Documentation" className="grid gap-7 text-[0.9375rem]">
      {groups.map((g) => (
        <div key={g.name}>
          <p className="t-label">{g.name}</p>
          <ul className="mt-2.5 grid gap-0.5 border-l border-line">
            {g.items.map((i) => {
              const on = i === current;
              return (
                <li key={i.href}>
                  <Link
                    href={i.href}
                    aria-current={on ? "page" : undefined}
                    className={cn(
                      "-ml-px flex min-h-9 items-center border-l-2 py-1 pl-4 pr-2 transition-colors",
                      on ? "border-highlight font-medium text-ink" : "border-transparent text-ink-2 hover:border-line-strong hover:text-ink",
                    )}
                  >
                    {i.title}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </nav>
  );
  return (
    <>
      <details className="rounded-md border border-line bg-surface lg:hidden">
        <summary className="flex min-h-12 cursor-pointer items-center justify-between px-4 font-medium">
          <span>
            <span className="t-label mr-2">Docs</span>
            {current?.title ?? "All pages"}
          </span>
          <span aria-hidden className="text-ink-2">▾</span>
        </summary>
        <div className="border-t border-line p-4">{list}</div>
      </details>
      <div className="hidden lg:block">{list}</div>
    </>
  );
}
