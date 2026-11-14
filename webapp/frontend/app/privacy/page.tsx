import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const metadata: Metadata = {
  title: "Privacy",
  description: "What the runtime and this website collect: no account, no telemetry, no trackers and no cookies.",
};

export default async function PrivacyPage() {
  const app = await renderMarkdown(readGenerated("privacy-and-security.md"), "md");
  return (
    <PageShell
      eyebrow="Privacy"
      title="Privacy of the runtime and of this site"
      lead="Neither the runtime nor this website has an account, an analytics service or a cookie. Here is what each one stores and where."
    >
      <DocSection id="runtime" title="The runtime">
        {app}
      </DocSection>
      <DocSection id="website" title="This website">
        <ul>
          <li>
            <strong>No trackers, no analytics, no cookies.</strong> The site loads no third-party script, font or image. Fonts are served
            from the site itself.
          </li>
          <li>
            <strong>One setting in your browser.</strong> If you choose light or dark, the choice is kept in your browser&apos;s local
            storage under the key <code>theme</code>. It never leaves your browser. Clear it to go back to your system setting.
          </li>
          <li>
            <strong>Search runs in your browser.</strong> The search box downloads one index file from this site and searches it locally.
            What you type is not sent anywhere.
          </li>
          <li>
            <strong>Operating system check.</strong> The download page reads your browser&apos;s platform hint to say whether this computer
            can run the runtime. The result is shown to you and not stored or sent.
          </li>
          <li>
            <strong>Policy still open.</strong> Whether the site will ever collect anonymous usage data is open decision W7 (DEC-027). Until
            it is decided, the answer is no.
          </li>
        </ul>
      </DocSection>
    </PageShell>
  );
}
