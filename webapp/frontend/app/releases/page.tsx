import type { Metadata } from "next";
import Link from "next/link";
import { PageShell } from "@/components/site/PageShell";
import { Status } from "@/components/ui/Status";
import { formatSize, loadManifest, sortReleases } from "@/lib/releases";

export const metadata: Metadata = {
  title: "Releases",
  description: "Published releases with their files and checksums, and what the first release brings.",
};

/** What the first release brings, in product terms. */
const highlights = [
  { title: "Setup from the project", body: "Reads your project and installs only what it still needs: JDK, Android SDK, emulator image, Flutter, Node and Git, all verified." },
  { title: "A phone beside your code", body: "A live, interactive emulator in a phone frame, docked next to your editor or agent, with an in-display fingerprint sensor." },
  { title: "Four frameworks", body: "Kotlin, Java, Flutter and React Native / Expo, each built with its own tools; saved files apply themselves." },
  { title: "Problems you can act on", body: "Build errors, runtime errors and crashes, classified and located at file and line, in the app and in VS Code." },
  { title: "Agents that check their work", body: "Claude Code, Codex, Cursor, Claude Desktop and VS Code connect over MCP, with permissions, approvals and an audit log." },
  { title: "Updates in place", body: "A new version installs over the old one with one click; your settings, toolchains and emulators stay." },
];

export default function ReleasesPage() {
  const releases = sortReleases(loadManifest().releases);
  return (
    <PageShell
      eyebrow="Releases"
      title="Releases"
      lead="Every published version with its files and checksums. The list comes from the same release manifest the app's updater reads."
    >
      <section aria-labelledby="published-title">
        <h2 id="published-title" className="t-section">
          Published releases
        </h2>
        {releases.length === 0 ? (
          <div className="mt-6 rounded-lg border border-dashed border-line-strong bg-bg-alt p-6">
            <Status kind="info">Coming soon</Status>
            <p className="t-body mt-4 max-w-[var(--reading-max)]">
              The first public release is being prepared. It will be published here and on the project&apos;s GitHub Releases page.
            </p>
          </div>
        ) : (
          <ol className="mt-8 grid gap-4">
            {releases.map((r) => (
              <li key={r.version} className="rounded-lg border border-line bg-surface p-6">
                <div className="flex flex-wrap items-center gap-3">
                  <h3 className="font-mono text-lg font-semibold">{r.version}</h3>
                  <Status kind="info">{r.channel}</Status>
                  <Status kind={r.signed ? "ok" : "warn"}>{r.signed ? "Code-signed" : "Not code-signed"}</Status>
                  <span className="t-meta t-num">{r.date}</span>
                </div>
                <ul className="mt-4 grid gap-1.5 text-sm">
                  {r.files.map((f) => (
                    <li key={f.name} className="flex flex-wrap gap-x-4">
                      <a href={f.url} className="link font-mono">{f.name}</a>
                      <span className="t-num text-ink-2">{formatSize(f.size)}</span>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
        )}
        <p className="t-desc mt-6">
          Checksums and the recommended file: <Link className="link" href="/download/">download page</Link>.
        </p>
      </section>

      <section aria-labelledby="highlights-title" className="mt-20 border-t border-line pt-14">
        <h2 id="highlights-title" className="t-section">
          What the first release brings
        </h2>
        <ul className="mt-10 grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {highlights.map((h) => (
            <li key={h.title} className="rounded-lg border border-line bg-surface p-6">
              <h3 className="font-semibold text-ink">{h.title}</h3>
              <p className="t-desc mt-2">{h.body}</p>
            </li>
          ))}
        </ul>
      </section>
    </PageShell>
  );
}
