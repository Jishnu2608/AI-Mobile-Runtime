import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const metadata: Metadata = {
  title: "Licences",
  description: "The product licence and the licences of the third-party software shipped with the runtime.",
};

export default async function LicencesPage() {
  const notices = await renderMarkdown(readGenerated("THIRD_PARTY_NOTICES.md"), "md");
  return (
    <PageShell
      eyebrow="Licences"
      title="Licences"
      lead="The licences of everything the runtime ships with, generated from the build."
    >
      <DocSection id="product" title="Product licence">
        <p>
          The runtime&apos;s own licence is published with the first public release. Until then, no licence is granted beyond what the
          third-party licences below allow for their own components.
        </p>
      </DocSection>
      <DocSection id="fonts" title="Website fonts">
        <p>This site uses Geist and Geist Mono by Vercel, under the SIL Open Font License 1.1, served from the site itself.</p>
      </DocSection>
      <DocSection id="third-party" title="Third-party software in the runtime">
        {notices}
      </DocSection>
    </PageShell>
  );
}
