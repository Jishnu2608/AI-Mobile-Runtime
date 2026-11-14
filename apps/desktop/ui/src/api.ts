// Bridge to the Runtime Core. Inside the desktop app calls go through Tauri commands; in a plain
// browser (UI development, screenshots) a mock with sample data answers instead.
import { invoke } from "@tauri-apps/api/core";
import { mockCall } from "./mock";
import { mockAgent } from "./mockAgents";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function call<T = any>(method: string, params: unknown = null): Promise<T> {
  if (!inTauri) return mockCall(method, params) as Promise<T>;
  return invoke<T>("core_call", { method, params });
}

/** `umr agent … --json` run by the desktop app (CON-005); a sample answer in a plain browser. */
export async function agentCli<T = any>(request: { action: string; client?: string; project?: string; scope?: string; remove?: boolean }): Promise<T> {
  const req = { ...request, project: request.project || undefined };
  if (!inTauri) return mockAgent(req) as Promise<T>;
  return invoke<T>("agent_cli", { request: req });
}

export type Step = "scan" | "project" | "plan" | "licences" | "install" | "ready" | "run" | "agents";

export type DocsPage = "getting-started" | "agents" | "troubleshooting" | "uninstall-and-data" | "privacy-and-security" | "cli-reference";

/** The user guides (GitHub until the website has a domain, W2); the app opens fixed pages only. */
const DOCS_BASE = "https://github.com/Jishnu2608/AI-Mobile-Runtime/blob/main/docs/user/";

/** Opens a guide page in the browser. */
export async function openDocs(page: DocsPage, anchor?: string): Promise<void> {
  if (!inTauri) {
    window.open(`${DOCS_BASE}${page}.md${anchor ? `#${anchor}` : ""}`, "_blank", "noopener");
    return;
  }
  await invoke("open_docs", { page, anchor: anchor ?? null });
}

/** Opens the docked live preview window. */
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

const KEY = "umr-wizard";

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
