import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";
import { site } from "@/lib/site";

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
          The runtime is licensed under the{" "}
          <a href={`${site.repository}/blob/main/LICENSE`} rel="noreferrer">Apache License, Version 2.0</a>. Its{" "}
          <a href={`${site.repository}/blob/main/NOTICE`} rel="noreferrer">NOTICE</a> file goes with every copy you pass on.
        </p>
        <p>
          If you use the runtime or build on it, a mention with a link to the repository would be greatly appreciated. That is a
          request, not a condition of the licence.
        </p>
      </DocSection>
      <DocSection id="trademarks" title="Name and logo">
        <p>
          The Apache licence covers the code, not the name or the logo. You can name the project, say your work is based on it or
          works with it, and pass on unchanged copies. A fork or a modified build you distribute needs its own name and icon. The
          details are in the{" "}
          <a href={`${site.repository}/blob/main/TRADEMARKS.md`} rel="noreferrer">trademark policy</a>.
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
