import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowLeft, ArrowRight, ChevronRight } from "lucide-react";
import { docHref, docs, findDoc, readSource, toc } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const dynamicParams = false;

export function generateStaticParams() {
  return docs.map((d) => ({ slug: d.slug }));
}

type Props = { params: Promise<{ slug: string[] }> };

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const d = findDoc((await params).slug);
  return d ? { title: d.title, description: d.description } : {};
}

export default async function DocPage({ params }: Props) {
  const { slug } = await params;
  const doc = findDoc(slug);
  if (!doc) notFound();

  const source = readSource(doc);
  const items = toc(source);
  const content = await renderMarkdown(source, doc.source.format);
  const i = docs.indexOf(doc);
  const prev = docs[i - 1];
  const next = docs[i + 1];

  return (
    <div className="grid gap-10 xl:grid-cols-[minmax(0,1fr)_14rem] xl:gap-14">
      <article className="min-w-0">
        <nav aria-label="Breadcrumb">
          <ol className="flex flex-wrap items-center gap-1.5 text-sm text-ink-2">
            <li>
              <Link className="link" href="/docs/">Docs</Link>
            </li>
            <li aria-hidden>
              <ChevronRight className="size-3.5" />
            </li>
            <li>{doc.group}</li>
            <li aria-hidden>
              <ChevronRight className="size-3.5" />
            </li>
            <li aria-current="page" className="text-ink">{doc.title}</li>
          </ol>
        </nav>
        <header className="mt-6 max-w-[var(--reading-max)] border-b border-line pb-8">
          <h1 className="t-page">{doc.title}</h1>
          <p className="t-lead mt-4">{doc.description}</p>
          {doc.source.kind === "generated" && (
            <p className="t-meta mt-4">Same text as the user guide that ships in the repository.</p>
          )}
        </header>
        <div className="prose mt-8 max-w-[var(--reading-max)]">{content}</div>

        <nav aria-label="Previous and next page" className="mt-16 grid max-w-[var(--reading-max)] gap-4 border-t border-line pt-8 sm:grid-cols-2">
          {prev ? (
            <Link href={docHref(prev)} className="card-link group rounded-md border border-line bg-surface p-4">
              <span className="t-label flex items-center gap-1.5">
                <ArrowLeft aria-hidden className="size-3.5" /> Previous
              </span>
              <span className="mt-1 block font-medium text-ink">{prev.title}</span>
            </Link>
          ) : (
            <span />
          )}
          {next && (
            <Link href={docHref(next)} className="card-link group rounded-md border border-line bg-surface p-4 text-right sm:col-start-2">
              <span className="t-label flex items-center justify-end gap-1.5">
                Next <ArrowRight aria-hidden className="size-3.5" />
              </span>
              <span className="mt-1 block font-medium text-ink">{next.title}</span>
            </Link>
          )}
        </nav>
      </article>

      {items.length > 1 && (
        <aside className="hidden xl:block">
          <nav aria-label="On this page" className="sticky top-[calc(var(--header-h)+1.5rem)] text-sm">
            <p className="t-label">On this page</p>
            <ul className="mt-3 grid gap-1.5 border-l border-line">
              {items.map((t) => (
                <li key={t.id}>
                  <a href={`#${t.id}`} className={`-ml-px block border-l border-transparent py-0.5 text-ink-2 hover:border-ink hover:text-ink ${t.depth === 3 ? "pl-7" : "pl-4"}`}>
                    {t.text}
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        </aside>
      )}
    </div>
  );
}
