import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { Callout } from "@/components/docs/Callout";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const metadata: Metadata = {
  title: "Security",
  description: "How to report a vulnerability, what is in scope, and the latest security review of the runtime.",
};

export default async function SecurityPage() {
  const policy = await renderMarkdown(readGenerated("SECURITY.md"), "md");
  const review = await renderMarkdown(readGenerated("security-review.md"), "md");
  return (
    <PageShell
      eyebrow="Security"
      title="Security policy and review"
      lead="The runtime downloads and runs developer tools, opens a local API and gives agents control of a device. This page says how each is protected and how to report a problem."
    >
      <Callout type="warning" title="Reporting channel not open yet">
        The policy below names GitHub private vulnerability reporting. The source repository is private until the product licence is
        decided (open decision W3), so that channel is not available to the public yet.
      </Callout>
      <div className="mt-12">
        <DocSection id="policy" title="Policy">
          {policy}
        </DocSection>
        <DocSection id="review" title="Latest security review">
          {review}
        </DocSection>
      </div>
    </PageShell>
  );
}
