import type { Metadata } from "next";
import { PageShell, DocSection } from "@/components/site/PageShell";
import { Callout } from "@/components/docs/Callout";
import { readGenerated } from "@/lib/docs";
import { renderMarkdown } from "@/lib/mdx";

export const metadata: Metadata = {
  title: "Security",
  description: "How the runtime protects your computer, your code and your agents, and how to report a vulnerability.",
};

const protections = [
  {
    title: "Everything stays on your computer",
    body: "The runtime has no account, no cloud service and no telemetry. It goes online only to download the tools you approved, from their official publishers, and to check for updates when you ask.",
  },
  {
    title: "Verified downloads",
    body: "Every download comes over HTTPS from an allow-list of official hosts and is checked against its published SHA-256 before it is unpacked or run. The JDK, Node.js, Git and the Android tools must also carry their publisher's Windows signature.",
  },
  {
    title: "Your system is left as it is",
    body: "Toolchains go into the runtime's own folder. Your PATH, JAVA_HOME, ANDROID_HOME, Android Studio and existing SDKs are never changed. Administrator rights are requested only for two named Windows settings, each with its own prompt.",
  },
  {
    title: "Projects run only when you trust them",
    body: "Building a project runs its own scripts. Nothing builds until you trust the folder, and agents cannot trust folders for you.",
  },
  {
    title: "Agents get limited, audited access",
    body: "Each agent has its own token and permissions and never administrator rights. Running a new folder or switching the device asks you first, every call is recorded, and secrets are masked in everything an agent reads.",
  },
  {
    title: "A local API only you can reach",
    body: "The Runtime Core listens on a named pipe that only your Windows user can open, and every client needs a token. The live preview uses a loopback connection with single-use tokens.",
  },
];

export default async function SecurityPage() {
  const policy = await renderMarkdown(readGenerated("SECURITY.md"), "md");
  return (
    <PageShell
      eyebrow="Security"
      title="Security"
      lead="The runtime installs developer tools, runs your projects and lets coding agents use a device. Here is how each of these is protected, and how to tell us about a problem."
    >
      <DocSection id="protections" title="How the runtime protects you">
        <ul className="not-prose mt-2 grid list-none gap-4 pl-0 sm:grid-cols-2">
          {protections.map((p) => (
            <li key={p.title} className="m-0 list-none rounded-lg border border-line bg-surface p-5">
              <p className="font-semibold text-ink">{p.title}</p>
              <p className="mt-2 text-[0.95rem] text-ink-2">{p.body}</p>
            </li>
          ))}
        </ul>
      </DocSection>
      <Callout type="warning" title="Report vulnerabilities privately">
        Please do not describe a vulnerability in a public issue. Follow the steps below so it can be fixed before details are public.
      </Callout>
      <div className="mt-10">
        <DocSection id="policy" title="Reporting a vulnerability">
          {policy}
        </DocSection>
      </div>
    </PageShell>
  );
}
