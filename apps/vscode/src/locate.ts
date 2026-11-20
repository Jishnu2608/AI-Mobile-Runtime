// Where tumr.exe is: the setting, then PATH, then the installer's folder, then (for this repository's
// samples) the newest `target\{release,debug}\tumr.exe` above an open folder. No `vscode` import.
import { existsSync, statSync } from "node:fs";
import * as path from "node:path";

export function candidates(setting: string, env: NodeJS.ProcessEnv): string[] {
  const out: string[] = [];
  if (setting.trim()) out.push(setting.trim());
  for (const dir of (env.PATH ?? env.Path ?? "").split(path.delimiter)) {
    if (dir.trim()) out.push(path.join(dir.trim(), process.platform === "win32" ? "tumr.exe" : "tumr"));
  }
  if (env.LOCALAPPDATA) out.push(path.join(env.LOCALAPPDATA, "Toolchain Universal Mobile Runtime", "tumr.exe"));
  return out;
}

/** A developer build of the runtime in an ancestor of `folder` (the samples in the repository). */
export function devBuild(folder: string): string | undefined {
  let dir = path.resolve(folder);
  for (let i = 0; i < 6; i++) {
    const builds = ["release", "debug"]
      .map((b) => path.join(dir, "target", b, "tumr.exe"))
      .filter((p) => existsSync(p) && existsSync(path.join(dir, "Cargo.toml")));
    if (builds.length) {
      return builds.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
    }
    const up = path.dirname(dir);
    if (up === dir) break;
    dir = up;
  }
  return undefined;
}

export function findTumr(setting: string, env: NodeJS.ProcessEnv = process.env, folders: string[] = []): string | undefined {
  return candidates(setting, env).find((p) => existsSync(p)) ?? folders.map(devBuild).find((p) => p !== undefined);
}
