"use client";

import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { CornerDownLeft, FileText, Search as SearchIcon } from "lucide-react";

type Entry = { href: string; title: string; section: string; headings: string[]; text: string };

/**
 * Site search over a static index built with the site (/search-index.json). A progressive
 * enhancement: without script the button is a link to the docs index.
 */
const noop = () => () => {};

export function SearchButton() {
  const dialog = useRef<HTMLDialogElement>(null);
  // true once hydrated: before that (and without script) the control is a plain link
  const ready = useSyncExternalStore(noop, () => true, () => false);

  const open = useCallback(() => dialog.current?.showModal(), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        open();
      } else if (e.key === "/" && !(e.target as HTMLElement)?.closest("input, textarea, [contenteditable]")) {
        e.preventDefault();
        open();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <>
      {ready ? (
        <button
          type="button"
          onClick={open}
          className="inline-flex h-11 items-center gap-2 rounded-sm px-2.5 text-ink-2 transition-colors hover:bg-sunken hover:text-ink md:border md:border-line md:bg-surface md:pr-2 md:pl-3"
          aria-label="Search the docs"
        >
          <SearchIcon aria-hidden className="size-[18px]" />
          <span className="hidden text-sm md:inline">Search</span>
          <span className="kbd ml-3 hidden md:inline">Ctrl K</span>
        </button>
      ) : (
        <Link href="/docs/" className="inline-flex size-11 items-center justify-center rounded-sm text-ink-2 hover:bg-sunken" aria-label="Browse the docs">
          <SearchIcon aria-hidden className="size-[18px]" />
        </Link>
      )}
      <SearchDialog ref={dialog} />
    </>
  );
}

function score(e: Entry, terms: string[]) {
  let s = 0;
  const title = e.title.toLowerCase();
  const heads = e.headings.join(" ").toLowerCase();
  const text = e.text.toLowerCase();
  for (const t of terms) {
    if (title.includes(t)) s += 10;
    if (heads.includes(t)) s += 4;
    if (text.includes(t)) s += 1;
    else if (!title.includes(t) && !heads.includes(t)) return 0;
  }
  return s;
}

function SearchDialog({ ref }: { ref: React.RefObject<HTMLDialogElement | null> }) {
  const [index, setIndex] = useState<Entry[] | null>(null);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const router = useRouter();

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    const onOpen = () => {
      if (!index) {
        fetch("/search-index.json")
          .then((r) => r.json())
          .then(setIndex)
          .catch(() => setIndex([]));
      }
      requestAnimationFrame(() => input.current?.focus());
    };
    // `toggle` fires when showModal() opens it
    d.addEventListener("toggle", onOpen);
    return () => d.removeEventListener("toggle", onOpen);
  }, [ref, index]);

  const results = useMemo(() => {
    const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
    if (!index || terms.length === 0) return [];
    return index
      .map((e) => ({ e, s: score(e, terms) }))
      .filter((r) => r.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 8)
      .map((r) => r.e);
  }, [q, index]);

  function go(href: string) {
    ref.current?.close();
    setQ("");
    router.push(href);
  }

  return (
    <dialog
      ref={ref}
      aria-label="Search"
      className="m-auto mt-[12vh] w-[min(40rem,calc(100vw-2rem))] rounded-md border border-line bg-surface p-0 text-ink shadow-2 backdrop:bg-[rgb(16_27_22/0.45)] backdrop:backdrop-blur-[2px]"
      onClick={(e) => e.target === e.currentTarget && ref.current?.close()}
    >
      <div className="flex items-center gap-3 border-b border-line px-4">
        <SearchIcon aria-hidden className="size-[18px] text-ink-2" />
        <label htmlFor="site-search" className="sr-only">
          Search the docs
        </label>
        <input
          id="site-search"
          ref={input}
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter" && results[active]) {
              e.preventDefault();
              go(results[active].href);
            }
          }}
          placeholder="Search docs, commands, problems"
          autoComplete="off"
          role="combobox"
          aria-expanded={results.length > 0}
          aria-controls="site-search-results"
          aria-activedescendant={results[active] ? `sr-${active}` : undefined}
          className="h-14 flex-1 bg-transparent text-base outline-none placeholder:text-ink-3"
        />
        <span className="kbd">Esc</span>
      </div>
      <ul id="site-search-results" role="listbox" aria-label="Results" className="max-h-[50vh] overflow-y-auto p-2">
        {q && index && results.length === 0 && <li className="px-3 py-6 text-center text-sm text-ink-2">No pages match “{q}”.</li>}
        {!q && <li className="px-3 py-6 text-center text-sm text-ink-2">Type a command (umr run), a tool (screenshot) or an error message.</li>}
        {results.map((r, i) => (
          <li key={r.href} id={`sr-${i}`} role="option" aria-selected={i === active}>
            <button
              type="button"
              onMouseEnter={() => setActive(i)}
              onClick={() => go(r.href)}
              className="flex w-full items-start gap-3 rounded-sm px-3 py-2.5 text-left aria-selected:bg-sunken data-[active=true]:bg-sunken"
              data-active={i === active}
            >
              <FileText aria-hidden className="mt-0.5 size-4 shrink-0 text-accent" />
              <span className="min-w-0 flex-1">
                <span className="block font-medium text-ink">{r.title}</span>
                <span className="block truncate text-sm text-ink-2">{r.section}</span>
              </span>
              {i === active && <CornerDownLeft aria-hidden className="mt-1 size-3.5 text-ink-2" />}
            </button>
          </li>
        ))}
      </ul>
    </dialog>
  );
}
