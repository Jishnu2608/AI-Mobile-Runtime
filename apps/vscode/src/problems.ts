// Runtime problems (`diagnostics.list`) → where they go in VS Code's Problems panel. No `vscode`
// import: the extension turns these into `vscode.Diagnostic`s.
import * as path from "node:path";

export interface RuntimeProblem {
  fingerprint: string;
  category: string;
  origin: string;
  severity: string;
  source?: string;
  message: string;
  file?: string;
  line?: number;
  column?: number;
  count?: number;
}

export interface Placed {
  /** Absolute file path, or null when the problem has no file (shown on the project folder). */
  file: string | null;
  line: number; // 0-based
  column: number; // 0-based
  severity: "error" | "warning" | "info";
  message: string;
  source: string;
  code: string;
}

/**
 * The absolute path of a problem's file: compilers report `file:///D:/x%20y/A.kt`, `D:/x/A.kt`
 * or a path relative to the project (`lib/main.dart`).
 */
export function resolveFile(file: string | undefined, project: string): string | null {
  if (!file) return null;
  let f = file.trim();
  if (/^file:\/\//i.test(f)) {
    f = f.replace(/^file:\/\/\/?/i, "");
  }
  try {
    f = decodeURIComponent(f);
  } catch {
    // keep it as it is
  }
  f = f.replace(/\//g, path.sep === "\\" ? "\\" : "/");
  if (/^[a-zA-Z]:[\\/]/.test(f) || f.startsWith("\\\\") || path.isAbsolute(f)) return path.normalize(f);
  return project ? path.join(project, f) : null;
}

export function place(p: RuntimeProblem, project: string): Placed {
  const what = p.category === "crash" ? "crash" : p.category === "runtime" ? "runtime error" : `${p.category}`;
  const times = p.count && p.count > 1 ? ` (×${p.count})` : "";
  return {
    file: resolveFile(p.file, project),
    line: Math.max(0, (p.line ?? 1) - 1),
    column: Math.max(0, (p.column ?? 1) - 1),
    severity: p.severity === "error" ? "error" : p.severity === "warning" ? "warning" : "info",
    message: `${p.message}${times}`,
    source: `Toolchain Universal Mobile Runtime · ${what}${p.origin === "environment" ? " (environment)" : ""}`,
    code: p.fingerprint,
  };
}
