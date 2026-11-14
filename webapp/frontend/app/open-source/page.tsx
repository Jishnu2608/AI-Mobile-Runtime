import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { Status } from "@/components/ui/Status";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";
import { pendingDecisions } from "@/lib/site";

export const metadata: Metadata = {
  title: "Open source",
  description: "The decisions still open before the public release, how to contribute and the code of conduct.",
};

export default async function OpenSourcePage() {
  const contributing = await renderMarkdown(readGenerated("CONTRIBUTING.md"), "md");
  const conduct = await renderMarkdown(readGenerated("CODE_OF_CONDUCT.md"), "md");
  return (
    <PageShell
      eyebrow="Open source"
      title="Open source and open decisions"
      lead="The repository is private until the product licence is chosen. These are the decisions still to be made before the public release, and the rules contributions will follow."
    >
      <section aria-labelledby="open-decisions-title" id="open-decisions" className="scroll-mt-28">
        <h2 id="open-decisions-title" className="t-section">
          Open decisions
        </h2>
        <p className="t-body mt-4 max-w-[var(--reading-max)]">
          Until each is decided, this site shows a labelled placeholder instead of a guess: a working name, no domain, no licence, no
          hosting and no signature.
        </p>
        <div className="table-wrap mt-8" role="region" aria-label="Open decisions" tabIndex={0}>
          <table className="w-full min-w-[34rem] text-left text-[0.95rem]">
            <thead>
              <tr className="border-b border-line-strong">
                <th scope="col" className="t-label py-3 pr-6 font-medium">Item</th>
                <th scope="col" className="t-label py-3 pr-6 font-medium">Decision</th>
                <th scope="col" className="t-label py-3 font-medium">Status</th>
              </tr>
            </thead>
            <tbody>
              {pendingDecisions.map((d) => (
                <tr key={d.w} className="border-b border-line">
                  <th scope="row" className="py-3 pr-6 font-mono font-semibold">
                    {d.w} <span className="font-normal text-ink-2">{d.id}</span>
                  </th>
                  <td className="py-3 pr-6 text-ink">{d.what}</td>
                  <td className="py-3">
                    <Status kind="pending">Pending decision</Status>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
      <div className="mt-16">
        <DocSection id="contributing" title="Contributing">
          {contributing}
        </DocSection>
        <DocSection id="code-of-conduct" title="Code of conduct">
          {conduct}
        </DocSection>
      </div>
    </PageShell>
  );
}
