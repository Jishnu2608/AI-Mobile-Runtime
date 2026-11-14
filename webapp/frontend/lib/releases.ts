import { readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";

/**
 * The release manifest (`releases.json`, schema 1) written by `scripts/release.ps1` — the same
 * file the in-app updater reads (DIST-002 / REL-002), so the download page cannot disagree with it.
 * Read at build time. `RELEASES_MANIFEST` may point the build at another manifest; the default is
 * `content/releases.json`, which lists no release until one is published (W5 hosting pending).
 */
export type ReleaseFile = { kind: "installer" | "portable" | "cli" | string; name: string; url: string; size: number; sha256: string };
export type Release = {
  version: string;
  channel: string;
  date: string;
  min_windows_build: number;
  signed: boolean;
  notes_url: string;
  files: ReleaseFile[];
};
export type Manifest = { schema: number; product: string; releases: Release[] };

const empty: Manifest = { schema: 1, product: "", releases: [] };

export function loadManifest(): Manifest {
  const p = process.env.RELEASES_MANIFEST;
  const path = p ? (isAbsolute(p) ? p : join(process.cwd(), p)) : join(process.cwd(), "content", "releases.json");
  try {
    const m = JSON.parse(readFileSync(path, "utf8").replace(/^﻿/, "")) as Manifest;
    if (m.schema !== 1 || !Array.isArray(m.releases)) return empty;
    // a file is offered only with a URL, a size and a checksum
    m.releases = m.releases
      .map((r) => ({ ...r, files: (r.files ?? []).filter((f) => f.url && f.size > 0 && /^[0-9a-f]{64}$/i.test(f.sha256)) }))
      .filter((r) => r.files.length > 0);
    return m;
  } catch {
    return empty;
  }
}

function key(v: string) {
  const [core, pre = ""] = v.replace(/^v/, "").split("-");
  const [a, b, c] = core.split(".").map((n) => Number(n) || 0);
  return [a, b, c, pre === "" ? 1 : 0, pre] as const;
}

export function sortReleases(rs: Release[]) {
  return rs.slice().sort((x, y) => {
    const a = key(x.version);
    const b = key(y.version);
    for (let i = 0; i < 4; i++) if (a[i] !== b[i]) return (b[i] as number) - (a[i] as number);
    return String(b[4]).localeCompare(String(a[4]));
  });
}

export function latestStable(m: Manifest): Release | null {
  return sortReleases(m.releases.filter((r) => r.channel === "stable"))[0] ?? null;
}

export function formatSize(bytes: number) {
  return bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

export const fileLabel: Record<string, { title: string; detail: string }> = {
  installer: { title: "Installer", detail: "Per-user install, no administrator rights. Adds a Start menu entry and an uninstaller." },
  portable: { title: "Portable ZIP", detail: "The same four programs; unpack and run. Settings still go to %USERPROFILE%\\.umr." },
  cli: { title: "Command line only", detail: "umr and the Runtime Core without the desktop app, for CI and headless machines." },
};
