import type { Metadata } from "next";
import Link from "next/link";
import { Download, FileArchive, PackageCheck, Terminal } from "lucide-react";
import { OsNotice } from "@/components/download/OsNotice";
import { Status } from "@/components/ui/Status";
import { CodeBlock } from "@/components/docs/CodeBlock";
import { fileLabel, formatSize, latestStable, loadManifest, sortReleases, type Release, type ReleaseFile } from "@/lib/releases";

export const metadata: Metadata = {
  title: "Download",
  description: "Download the runtime for Windows 11 x64: version, date, size, SHA-256 checksum and signature status of every file.",
};

const icons = { installer: PackageCheck, portable: FileArchive, cli: Terminal } as Record<string, typeof Download>;

function date(d: string) {
  return new Date(`${d}T00:00:00Z`).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
}

export default function DownloadPage() {
  const manifest = loadManifest();
  const latest = latestStable(manifest);
  const older = sortReleases(manifest.releases).filter((r) => r !== latest);

  return (
    <div className="wrap pb-24 pt-14 md:pt-20">
      <header className="max-w-3xl">
        <p className="eyebrow">Download</p>
        <h1 className="t-page mt-5">Download for Windows 11</h1>
        <p className="t-lead mt-5">
          One per-user installer, no administrator rights. The toolchain itself downloads later, inside the app, after you approve the plan.
        </p>
        <p className="mt-6 text-sm font-medium text-ink">Requires Windows 11 x64, build 22000 or later.</p>
        <OsNotice />
      </header>

      {latest ? <Latest release={latest} /> : <NoRelease />}

      <section aria-labelledby="verify-title" className="mt-20 grid gap-8 border-t border-line pt-14 lg:grid-cols-12">
        <div className="lg:col-span-4">
          <h2 id="verify-title" className="t-sub">
            Check the file before you run it
          </h2>
          <p className="t-desc mt-3">
            Compare the SHA-256 checksum PowerShell prints with the one on this page. If they differ, delete the file and do not run it.
          </p>
        </div>
        <div className="lg:col-span-8">
          <CodeBlock data-language="powershell">
            <code>{`Get-FileHash .\\${latest?.files.find((f) => f.kind === "installer")?.name ?? "<downloaded file>"} -Algorithm SHA256`}</code>
          </CodeBlock>
          <p className="t-desc mt-4">
            Signature: builds are not code-signed yet (open decision W8), so Windows SmartScreen may warn on first run. The checksum is the
            check that applies until signing is in place.
          </p>
        </div>
      </section>

      <section aria-labelledby="archive-title" className="mt-20 border-t border-line pt-14">
        <h2 id="archive-title" className="t-sub">
          Earlier releases
        </h2>
        {older.length === 0 ? (
          <p className="t-desc mt-3">No earlier releases.</p>
        ) : (
          <div className="table-wrap mt-6" role="region" aria-label="Earlier releases" tabIndex={0}>
            <table className="w-full min-w-[36rem] text-left text-[0.95rem]">
              <thead>
                <tr className="border-b border-line-strong">
                  <th scope="col" className="t-label py-3 pr-6 font-medium">Version</th>
                  <th scope="col" className="t-label py-3 pr-6 font-medium">Date</th>
                  <th scope="col" className="t-label py-3 font-medium">Files</th>
                </tr>
              </thead>
              <tbody>
                {older.map((r) => (
                  <tr key={r.version} className="border-b border-line align-top">
                    <th scope="row" className="py-3 pr-6 font-mono font-semibold">{r.version}</th>
                    <td className="t-num py-3 pr-6 text-ink-2">{date(r.date)}</td>
                    <td className="py-3">
                      {r.files.map((f) => (
                        <a key={f.name} href={f.url} className="link mr-4 font-mono text-sm">
                          {f.name}
                        </a>
                      ))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <p className="t-desc mt-6">
          What changed in each version: <Link className="link" href="/releases/">release notes</Link>.
        </p>
      </section>
    </div>
  );
}

function Latest({ release }: { release: Release }) {
  const installer = release.files.find((f) => f.kind === "installer") ?? release.files[0];
  const others = release.files.filter((f) => f !== installer);
  return (
    <>
      <section aria-labelledby="latest-title" className="dl-gate mt-14 rounded-lg border border-line bg-surface p-6 shadow-1 sm:p-8">
        <div className="flex flex-wrap items-center gap-3">
          <Status kind="ok">Recommended</Status>
          <Status kind={release.signed ? "ok" : "warn"}>{release.signed ? "Code-signed" : "Not code-signed"}</Status>
        </div>
        <h2 id="latest-title" className="t-section mt-5">
          {fileLabel.installer.title} <span className="font-mono text-ink-2">{release.version}</span>
        </h2>
        <p className="t-body mt-3 max-w-2xl">{fileLabel.installer.detail}</p>
        <FileFacts file={installer} release={release} />

        <div className="mt-8 rounded-md border border-line bg-bg-alt p-5">
          <p className="font-semibold text-ink">Licence</p>
          <p className="t-desc mt-2">
            The product licence is not chosen yet (open decision W3). The components the app installs come with their own licences, which
            the setup wizard shows you one by one before anything downloads. The ones shipped inside the app are on the{" "}
            <Link className="link" href="/licences/">licences page</Link>.
          </p>
          <label className="mt-4 flex min-h-11 cursor-pointer items-start gap-3 text-[0.95rem] text-ink">
            <input type="checkbox" className="mt-1 size-4 accent-[var(--pine)]" />
            <span>I have read the licence notes and understand that this build is a preview without a product licence.</span>
          </label>
        </div>

        <div className="mt-6 flex flex-wrap items-center gap-4">
          <span className="dl-locked btn btn-primary h-12 px-5 text-base" aria-disabled="true">
            <Download aria-hidden className="size-4" />
            Download {installer.name}
          </span>
          <a className="dl-open btn btn-primary h-12 px-5 text-base" href={installer.url} download>
            <Download aria-hidden className="size-4" />
            Download {installer.name}
          </a>
          <span className="dl-hint t-desc">Tick the licence box to enable the download.</span>
        </div>
      </section>

      <section aria-labelledby="alt-title" className="mt-14">
        <h2 id="alt-title" className="t-sub">
          Other files in {release.version}
        </h2>
        <ul className="mt-6 grid gap-4 md:grid-cols-2">
          {others.map((f) => {
            const Icon = icons[f.kind] ?? FileArchive;
            const label = fileLabel[f.kind] ?? { title: f.kind, detail: "" };
            return (
              <li key={f.name} className="flex flex-col rounded-lg border border-line bg-surface p-6">
                <Icon aria-hidden className="size-5 text-pine dark:text-jade" strokeWidth={1.75} />
                <h3 className="mt-4 font-semibold text-ink">{label.title}</h3>
                <p className="t-desc mt-2 flex-1">{label.detail}</p>
                <FileFacts file={f} release={release} compact />
                <a href={f.url} download className="btn mt-5 h-11 self-start px-4">
                  <Download aria-hidden className="size-4" />
                  Download {f.name}
                </a>
              </li>
            );
          })}
        </ul>
      </section>
    </>
  );
}

function FileFacts({ file, release, compact }: { file: ReleaseFile; release: Release; compact?: boolean }) {
  const rows: [string, React.ReactNode][] = [
    ["Version", <span key="v" className="font-mono">{release.version}</span>],
    ["Released", date(release.date)],
    ["Size", formatSize(file.size)],
    ["Operating system", `Windows 11 x64 (build ${release.min_windows_build}+)`],
  ];
  return (
    <dl className={compact ? "mt-4 grid gap-1.5 text-sm" : "mt-6 grid gap-x-8 gap-y-3 text-[0.95rem] sm:grid-cols-2 lg:grid-cols-4"}>
      {rows.map(([k, v]) => (
        <div key={k} className={compact ? "flex justify-between gap-4" : ""}>
          <dt className="t-label">{k}</dt>
          <dd className={compact ? "t-num text-ink" : "t-num mt-1 font-medium text-ink"}>{v}</dd>
        </div>
      ))}
      <div className={compact ? "mt-1" : "sm:col-span-2 lg:col-span-4"}>
        <dt className="t-label">SHA-256</dt>
        <dd className="mt-1 break-all font-mono text-[0.8125rem] text-ink">{file.sha256}</dd>
      </div>
    </dl>
  );
}

function NoRelease() {
  return (
    <section aria-labelledby="none-title" className="mt-14 rounded-lg border border-dashed border-line-strong bg-bg-alt p-6 sm:p-10">
      <Status kind="pending">No public release yet</Status>
      <h2 id="none-title" className="t-section mt-5 max-w-2xl">
        There is nothing to download yet
      </h2>
      <p className="t-body mt-4 max-w-2xl">
        Release builds exist, but where they are published is not decided (open decision W5), and they are not code-signed yet (W8). When
        the first release is published, this page lists it from the same release manifest the app&apos;s updater reads.
      </p>
      <p className="mt-8 font-semibold text-ink">Each release will show</p>
      <ul className="mt-3 grid gap-2 text-[0.95rem] text-ink-2 sm:grid-cols-2 lg:grid-cols-3">
        {["Version and release date", "File size", "SHA-256 checksum", "Signature status", "Supported Windows build", "Installer, portable ZIP and command-line ZIP"].map((x) => (
          <li key={x} className="flex items-center gap-2">
            <span aria-hidden className="size-1.5 rounded-full bg-emerald" />
            {x}
          </li>
        ))}
      </ul>
      <div className="mt-8 flex flex-wrap gap-3">
        <Link href="/docs/requirements/" className="btn h-11 px-5">Check the requirements</Link>
        <Link href="/releases/" className="btn btn-ghost h-11 px-5">See the release timeline</Link>
      </div>
    </section>
  );
}
