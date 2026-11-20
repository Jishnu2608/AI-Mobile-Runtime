// Bridge to the Runtime Core. Inside the desktop app calls go through Tauri commands; in a plain
// browser (UI development, screenshots) a mock with sample data answers instead.
import { invoke } from "@tauri-apps/api/core";
import { mockCall } from "./mock";
import { mockAgent } from "./mockAgents";

// VSC-001: inside the VS Code extension's Mobile Preview panel the page talks to the extension,
// which forwards to the Runtime Core (`tumr rpc --stdio`).
interface VsCodeApi {
  postMessage(m: unknown): void;
}
declare const acquireVsCodeApi: undefined | (() => VsCodeApi);
const vscode: VsCodeApi | null = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : null;
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: unknown) => void }>();
let nextId = 1;
if (vscode) {
  window.addEventListener("message", (ev: MessageEvent) => {
    const m = ev.data as { type?: string; id?: number; result?: unknown; error?: string };
    if (m?.type !== "rpc" || m.id === undefined) return;
    const p = pending.get(m.id);
    if (!p) return;
    pending.delete(m.id);
    if (m.error !== undefined) p.reject(m.error);
    else p.resolve(m.result);
  });
}

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
/** Inside a real app (desktop window or VS Code panel), not the browser mock. */
export const inApp = inTauri || vscode !== null;

export async function call<T = any>(method: string, params: unknown = null): Promise<T> {
  if (vscode) {
    const id = nextId++;
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve, reject });
      vscode.postMessage({ type: "rpc", id, method, params });
    });
  }
  if (!inTauri) return mockCall(method, params) as Promise<T>;
  return invoke<T>("core_call", { method, params });
}

/** `tumr agent … --json` run by the desktop app (CON-005); a sample answer in a plain browser. */
export async function agentCli<T = any>(request: { action: string; client?: string; project?: string; scope?: string; remove?: boolean }): Promise<T> {
  const req = { ...request, project: request.project || undefined };
  if (!inTauri) return mockAgent(req) as Promise<T>;
  return invoke<T>("agent_cli", { request: req });
}

export type Step = "scan" | "project" | "plan" | "licences" | "install" | "ready" | "run" | "agents";

export type DocsPage = "getting-started" | "agents" | "troubleshooting" | "uninstall-and-data" | "privacy-and-security" | "cli-reference";

/** The user guides (GitHub until the website has a domain, W2); the app opens fixed pages only. */
const DOCS_BASE = "https://github.com/Jishnu2608/TUMR/blob/main/docs/user/";

/** Opens a guide page in the browser. */
export async function openDocs(page: DocsPage, anchor?: string): Promise<void> {
  if (vscode) {
    // the extension opens it in the browser (web views cannot open windows)
    vscode.postMessage({ type: "openDocs", url: `${DOCS_BASE}${page}.md${anchor ? `#${anchor}` : ""}` });
    return;
  }
  if (!inTauri) {
    window.open(`${DOCS_BASE}${page}.md${anchor ? `#${anchor}` : ""}`, "_blank", "noopener");
    return;
  }
  await invoke("open_docs", { page, anchor: anchor ?? null });
}

/**
 * Windows' own folder picker (the app's `pick_folder` command); `null` when cancelled. The page gets
 * the chosen path only, no file system access. In a plain browser: a sample path (UI development).
 */
export async function pickFolder(start?: string): Promise<string | null> {
  if (!inTauri) return "D:\\Work\\my-app";
  return invoke<string | null>("pick_folder", { start: start ?? null });
}

/** UPD-001: a newer version on the configured update channel (`available` null: up to date). */
export interface UpdateInfo {
  current: string;
  configured?: boolean;
  available?: { version: string; date?: string; notes_url?: string; signed?: boolean } | null;
}

export async function checkUpdate(): Promise<UpdateInfo> {
  if (!inTauri) {
    // `?update=1` shows the banner in a plain browser
    const demo = new URLSearchParams(window.location.search).get("update") === "1";
    return { current: "0.1.0", available: demo ? { version: "0.1.1", notes_url: "" } : null };
  }
  return invoke<UpdateInfo>("update_check");
}

/** Downloads, verifies and installs the update; the app closes and opens again by itself. */
export async function applyUpdate(): Promise<void> {
  if (!inTauri) return;
  await invoke("update_apply");
}

/** Opens the docked live preview window. */
/** AUD-P1-5: the window's own title (taskbar, Alt+Tab); the page title too. */
/** AUD-P2-20 / AUD-P3-23: open a file (at a line) in VS Code, else Notepad. Returns the program used. */
export async function openInEditor(path: string, line?: number | null, root?: string): Promise<string> {
  if (!inTauri) throw new Error("opening files works in the desktop app");
  return invoke<string>("open_in_editor", { path, line: line ?? null, root: root ?? null });
}

/** AUD-P2-21: grow (or shrink) this window sideways; returns the logical pixels actually added. */
export async function widenWindow(extra: number): Promise<number> {
  if (!inTauri) return 0;
  return invoke<number>("widen_window", { extra });
}

export async function setWindowTitle(title: string): Promise<void> {
  document.title = title;
  if (inTauri) await invoke("set_window_title", { title }).catch(() => undefined);
}

/** AUD-P2-26: open (or bring up) the preview with its Problems panel open. Both windows share
 * the web view's storage, so the preview also hears this when it is already open. */
export async function openProblems(): Promise<void> {
  try {
    localStorage.setItem("tumr.openProblems", String(Date.now()));
  } catch {
    /* the preview opens without its panel */
  }
  await openPreview();
}

export async function openPreview(): Promise<void> {
  if (!inTauri) return;
  await invoke("open_preview_window");
}

/** Troubleshooting section for a problem category (docs/user/troubleshooting.md headings). */
export function troubleshootingAnchor(category: string): string {
  return (
    { build: "build", runtime: "runtime-and-crash", crash: "runtime-and-crash", environment: "environment", network: "network", device: "device" } as Record<string, string>
  )[category] ?? "";
}

export interface WizardState {
  step: Step;
  projectPath: string;
  jobId?: string;
}

const KEY = "tumr-wizard";

export async function loadWizard(): Promise<WizardState | null> {
  if (!inTauri) {
    try {
      const t = localStorage.getItem(KEY);
      return t ? (JSON.parse(t) as WizardState) : null;
    } catch {
      return null;
    }
  }
  return invoke<WizardState | null>("wizard_load");
}

export async function saveWizard(state: WizardState): Promise<void> {
  if (!inTauri) {
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch {
      /* private mode: not saved */
    }
    return;
  }
  await invoke("wizard_save", { state });
}

export function size(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  return `${Math.round(bytes / 1048576)} MB`;
}
