import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";
import { site } from "@/lib/site";

export const metadata: Metadata = {
  title: "Open source",
  description: "Where the source code is, how to contribute and the code of conduct.",
};

export default async function OpenSourcePage() {
  const contributing = await renderMarkdown(readGenerated("CONTRIBUTING.md"), "md");
  const conduct = await renderMarkdown(readGenerated("CODE_OF_CONDUCT.md"), "md");
  return (
    <PageShell
      eyebrow="Open source"
      title="Open source"
      lead="The source code of the runtime is public on GitHub. This page explains how to contribute and the rules every contribution follows."
    >
      <DocSection id="source" title="Source code">
        <p>
          The repository holds the Runtime Core, the command line, the desktop app, the VS Code extension, this website and the user
          guides: <a href={site.repository} rel="noreferrer">{site.repository.replace("https://", "")}</a>.
        </p>
        <p>The licence is published with the first public release.</p>
      </DocSection>
      <DocSection id="contributing" title="Contributing">
        {contributing}
      </DocSection>
      <DocSection id="code-of-conduct" title="Code of conduct">
        {conduct}
      </DocSection>
    </PageShell>
  );
}
