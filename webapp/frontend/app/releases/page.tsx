import type { Metadata } from "next";
import Link from "next/link";
import { PageShell } from "@/components/site/PageShell";
import { Status } from "@/components/ui/Status";
import { formatSize, loadManifest, sortReleases } from "@/lib/releases";

export const metadata: Metadata = {
  title: "Releases",
  description: "Published releases with their files and checksums, and the development milestones reached so far.",
};

/** Development milestones from the build plan. These are not releases: nothing here was published. */
const milestones = [
  { id: "M0", date: "2026-11-11", title: "Proof of concept: go", body: "Provisioning, preview stream and framework adapters proven feasible on Windows 11." },
  { id: "M1", date: "2026-11-12", title: "Clean Windows to a ready SDK", body: "Environment scan, verified installs, licence flow, repair and the setup wizard." },
  { id: "M2", date: "2026-11-12", title: "Emulator boots from the runtime", body: "Six device profiles; large profile cold boot 27.6 s, warm 5.8 s; 100 of 100 boot loops healthy." },
  { id: "M3", date: "2026-11-13", title: "Kotlin, Flutter and Expo templates run", body: "One command runs each template; Flutter hot reload 0.67 to 1.8 s, Expo Fast Refresh 0.36 to 0.50 s." },
  { id: "M4", date: "2026-11-13", title: "Docked interactive preview", body: "40 to 42 frames per second; input to screen p50 51 to 72 ms." },
  { id: "M5", date: "2026-11-13", title: "Problems panel", body: "Six categories; 76 seeded failures classified, category 94.7 % on the first run." },
  { id: "M6", date: "2026-11-13", title: "Agents over MCP", body: "Claude Code fixed a seeded Flutter bug alone through 16 MCP tools. Codex is still to be checked." },
  { id: "M8", date: "2026-11-14", title: "Release build ready, four items open", body: "Installer, updater, security review and docs are done; signing, clean-VM test, licence and the 1.0 tag are open." },
];

export default function ReleasesPage() {
  const releases = sortReleases(loadManifest().releases);
  return (
    <PageShell
      eyebrow="Releases"
      title="Releases and milestones"
      lead="Published releases come from the same manifest the app's updater reads. Below them are the development milestones reached so far, which are not releases."
    >
      <section aria-labelledby="published-title">
        <h2 id="published-title" className="t-section">
          Published releases
        </h2>
        {releases.length === 0 ? (
          <div className="mt-6 rounded-lg border border-dashed border-line-strong bg-bg-alt p-6">
            <Status kind="pending">None yet</Status>
            <p className="t-body mt-4 max-w-[var(--reading-max)]">
              No release has been published. Where releases are hosted is open decision W5, and signing is W8.
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

      <section aria-labelledby="milestones-title" className="mt-20 border-t border-line pt-14">
        <h2 id="milestones-title" className="t-section">
          Development milestones
        </h2>
        <p className="t-body mt-4 max-w-[var(--reading-max)]">Measured on the development machine. Milestone M7 (macOS and iOS) is not planned in this release.</p>
        <ol className="relative mt-10 grid gap-8 border-l border-line pl-8">
          {milestones.map((m) => (
            <li key={m.id} className="relative">
              <span aria-hidden className="absolute -left-[2.3rem] top-1.5 size-2.5 rounded-full border-2 border-bg bg-emerald ring-1 ring-line" />
              <p className="t-meta t-num">
                <span className="font-mono font-semibold text-ink">{m.id}</span> · {m.date}
              </p>
              <h3 className="mt-1 font-semibold text-ink">{m.title}</h3>
              <p className="t-desc mt-1 max-w-2xl">{m.body}</p>
            </li>
          ))}
        </ol>
      </section>
    </PageShell>
  );
}
