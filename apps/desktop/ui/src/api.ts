// Bridge to the Runtime Core. Inside the desktop app calls go through Tauri commands; in a plain
// browser (UI development, screenshots) a mock with sample data answers instead.
import { invoke } from "@tauri-apps/api/core";
import { mockCall } from "./mock";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function call<T = any>(method: string, params: unknown = null): Promise<T> {
  if (!inTauri) return mockCall(method, params) as Promise<T>;
  return invoke<T>("core_call", { method, params });
}

export type Step = "scan" | "project" | "plan" | "licences" | "install" | "ready";

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
