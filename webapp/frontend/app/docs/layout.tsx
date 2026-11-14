import { DocsNav } from "@/components/docs/DocsNav";
import { docGroups, docHref, docs } from "@/lib/docs";

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  const groups = docGroups.map((g) => ({
    name: g,
    items: docs.filter((d) => d.group === g).map((d) => ({ href: docHref(d), title: d.title })),
  }));
  return (
    <div className="wrap grid gap-8 pb-24 pt-8 lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12 lg:pt-12">
      <aside className="lg:sticky lg:top-[calc(var(--header-h)+1.5rem)] lg:max-h-[calc(100dvh-var(--header-h)-3rem)] lg:self-start lg:overflow-y-auto lg:pb-8">
        <DocsNav groups={groups} />
      </aside>
      <div className="min-w-0">{children}</div>
    </div>
  );
}
