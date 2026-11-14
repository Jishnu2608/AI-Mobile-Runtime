import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { Status } from "@/components/ui/Status";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const metadata: Metadata = {
  title: "Licences",
  description: "The product licence (not decided yet) and the licences of the third-party software shipped with the runtime.",
};

export default async function LicencesPage() {
  const notices = await renderMarkdown(readGenerated("THIRD_PARTY_NOTICES.md"), "md");
  return (
    <PageShell
      eyebrow="Licences"
      title="Licences"
      lead="The runtime's own licence is not chosen yet. The licences of everything it ships with are listed below, generated from the build."
    >
      <DocSection id="product" title="Product licence">
        <p>
          <Status kind="pending">Pending decision W3</Status>
        </p>
        <p>
          The licence of the runtime itself (open decision W3, DEC-025) has not been chosen. Until it is, no licence is granted beyond what
          the third-party licences below allow for their own components.
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
