// VSC-001: AI Mobile Runtime for VS Code.
//
// * Run on Emulator: `umr run --no-preview <folder>` as a task (its output and the trust question
//   in VS Code's terminal), then the Mobile Preview panel beside the editor.
// * Reload / Restart / Rebuild / Stop and the status bar: Core API calls through the bridge.
// * Problems: the run's build errors, runtime errors and crashes in the Problems panel.
// * Connect Copilot: `umr agent setup vscode` (MCP tools + instructions for agent mode).
// Saved files reload by themselves: the Runtime Core watches the project.
import * as path from "node:path";
import * as vscode from "vscode";
import { CoreClient } from "./core";
import { findUmr } from "./locate";
import { PreviewPanel } from "./preview";
import { place, RuntimeProblem } from "./problems";

let core: CoreClient | undefined;
let coreExe = "";

function umrExe(): string | undefined {
  const setting = vscode.workspace.getConfiguration("umr").get<string>("path") ?? "";
  const folders = (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath);
  return findUmr(setting, process.env, folders);
}

function getCore(): CoreClient | undefined {
  const exe = umrExe();
  if (!exe) return undefined;
  if (!core || exe !== coreExe) {
    core?.dispose();
    core = new CoreClient(exe);
    coreExe = exe;
  }
  return core;
}

async function requireUmr(): Promise<string | undefined> {
  const exe = umrExe();
  if (exe) return exe;
  const pick = await vscode.window.showErrorMessage(
    "AI Mobile Runtime: umr.exe was not found (not on PATH, not in the default install folder).",
    "Locate umr.exe",
  );
  if (pick) return locate();
  return undefined;
}

async function locate(): Promise<string | undefined> {
  const r = await vscode.window.showOpenDialog({
    canSelectMany: false,
    openLabel: "Use this umr.exe",
    filters: { Programs: ["exe"] },
  });
  if (!r?.length) return undefined;
  await vscode.workspace.getConfiguration("umr").update("path", r[0].fsPath, vscode.ConfigurationTarget.Global);
  return r[0].fsPath;
}

async function pickFolder(): Promise<string | undefined> {
  const folders = vscode.workspace.workspaceFolders ?? [];
  if (folders.length === 1) return folders[0].uri.fsPath;
  if (folders.length === 0) {
    vscode.window.showErrorMessage("Open your app's folder first (File › Open Folder).");
    return undefined;
  }
  const f = await vscode.window.showWorkspaceFolderPick({ placeHolder: "Which app should run?" });
  return f?.uri.fsPath;
}

/** Runs `umr <args>` as a task in VS Code's terminal (interactive: trust and agent questions). */
async function umrTask(name: string, args: string[], folder: string): Promise<void> {
  const exe = await requireUmr();
  if (!exe) return;
  const task = new vscode.Task(
    { type: "umr", task: name },
    vscode.TaskScope.Workspace,
    name,
    "AI Mobile Runtime",
    new vscode.ProcessExecution(exe, args, { cwd: folder }),
  );
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated, clear: true };
  await vscode.tasks.executeTask(task);
}

async function coreCall(label: string, method: string, params: unknown): Promise<any> {
  const c = getCore();
  if (!c) {
    await requireUmr();
    return undefined;
  }
  try {
    return await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `AI Mobile Runtime: ${label}…` },
      () => c.call(method, params),
    );
  } catch (e) {
    vscode.window.showErrorMessage(`AI Mobile Runtime: ${label} failed: ${(e as Error).message}`);
    return undefined;
  }
}

async function reload(kind: "hot" | "restart" | "full", label: string): Promise<void> {
  const r = await coreCall(label, "project.reload", { kind });
  if (!r) return;
  if (r.status === "unsupported") {
    vscode.window.showInformationMessage(`AI Mobile Runtime: ${r.message ?? "this app does not support that reload"}`);
  } else if (r.status !== "reloaded") {
    vscode.window.showWarningMessage(`AI Mobile Runtime: ${label}: ${r.error ?? r.status} — see the Problems panel.`);
  } else {
    vscode.window.setStatusBarMessage(`$(check) ${label} in ${r.ms} ms`, 4000);
  }
}

export function activate(context: vscode.ExtensionContext): void {
  vscode.commands.executeCommand("setContext", "umr.active", true);
  const diagnostics = vscode.languages.createDiagnosticCollection("umr");
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = "umr.menu";
  status.text = "$(device-mobile) Mobile";
  status.tooltip = "AI Mobile Runtime";
  status.show();
  context.subscriptions.push(diagnostics, status, { dispose: () => core?.dispose() });

  const cmd = (id: string, f: () => unknown) => context.subscriptions.push(vscode.commands.registerCommand(id, f));

  cmd("umr.run", async () => {
    const folder = await pickFolder();
    if (!folder) return;
    await umrTask("Run on Emulator", ["run", "--no-preview", folder], folder);
    if (vscode.workspace.getConfiguration("umr").get<boolean>("openPreviewOnRun", true)) {
      PreviewPanel.show(context, getCore);
    }
  });
  cmd("umr.reload", () => reload("hot", "Reload"));
  cmd("umr.restart", () => reload("restart", "Restart"));
  cmd("umr.rebuild", () => reload("full", "Rebuild"));
  cmd("umr.stop", async () => {
    if (await coreCall("Stop", "project.stop", {})) vscode.window.setStatusBarMessage("$(debug-stop) App stopped", 3000);
  });
  cmd("umr.openPreview", async () => {
    if (await requireUmr()) PreviewPanel.show(context, getCore);
  });
  cmd("umr.connectCopilot", async () => {
    const folder = await pickFolder();
    if (folder) await umrTask("Connect Copilot", ["agent", "setup", "vscode", "--project", folder], folder);
  });
  cmd("umr.locate", locate);
  cmd("umr.menu", async () => {
    const items: (vscode.QuickPickItem & { id: string })[] = [
      { id: "umr.run", label: "$(play) Run on Emulator" },
      { id: "umr.openPreview", label: "$(device-mobile) Open Mobile Preview" },
      { id: "umr.reload", label: "$(refresh) Reload (hot)" },
      { id: "umr.restart", label: "$(debug-restart) Restart the App" },
      { id: "umr.rebuild", label: "$(tools) Rebuild and Reinstall" },
      { id: "umr.stop", label: "$(debug-stop) Stop the App" },
      { id: "umr.connectCopilot", label: "$(copilot) Connect Copilot (agent mode)" },
    ];
    const pick = await vscode.window.showQuickPick(items, { placeHolder: "AI Mobile Runtime" });
    if (pick) vscode.commands.executeCommand(pick.id);
  });

  // status bar and Problems: what runs, every 2 s while VS Code is focused
  let busy = false;
  const refresh = async () => {
    if (busy || !vscode.window.state.focused) return;
    const c = getCore();
    if (!c) {
      status.text = "$(device-mobile) Mobile: umr.exe not found";
      return;
    }
    busy = true;
    try {
      const s = await c.call("project.status", {}, 15_000);
      const run = s?.run;
      const mine = run && (vscode.workspace.workspaceFolders ?? []).some((f) => samePath(f.uri.fsPath, run.project));
      vscode.commands.executeCommand("setContext", "umr.running", !!mine);
      if (!run) {
        status.text = "$(device-mobile) Mobile: not running";
        status.tooltip = "AI Mobile Runtime — nothing runs. Click for actions.";
      } else {
        const alive = s.alive === false ? " (stopped)" : "";
        status.text = `$(device-mobile) ${run.package}${alive}`;
        status.tooltip = `AI Mobile Runtime — ${run.adapter} app on ${run.serial}. Click for actions.`;
      }
      if (vscode.workspace.getConfiguration("umr").get<boolean>("problems", true) && mine) {
        const d = await c.call("diagnostics.list", { limit: 200 }, 15_000);
        publish(diagnostics, (d?.entries ?? []) as RuntimeProblem[], run.project);
      } else {
        diagnostics.clear();
      }
    } catch {
      status.text = "$(device-mobile) Mobile: Runtime Core not reachable";
    } finally {
      busy = false;
    }
  };
  const timer = setInterval(refresh, 2000);
  context.subscriptions.push({ dispose: () => clearInterval(timer) });
  refresh();
}

function samePath(a: string, b: string): boolean {
  const n = (p: string) => path.normalize(p).replace(/[\\/]+$/, "").toLowerCase();
  return n(a) === n(b);
}

function publish(collection: vscode.DiagnosticCollection, problems: RuntimeProblem[], project: string): void {
  const byFile = new Map<string, vscode.Diagnostic[]>();
  for (const p of problems) {
    const at = place(p, project);
    const file = at.file ?? project;
    const range = new vscode.Range(at.line, at.column, at.line, at.column + 1000);
    const sev =
      at.severity === "error"
        ? vscode.DiagnosticSeverity.Error
        : at.severity === "warning"
          ? vscode.DiagnosticSeverity.Warning
          : vscode.DiagnosticSeverity.Information;
    const d = new vscode.Diagnostic(range, at.message, sev);
    d.source = at.source;
    d.code = at.code;
    const list = byFile.get(file) ?? [];
    list.push(d);
    byFile.set(file, list);
  }
  collection.clear();
  for (const [file, list] of byFile) collection.set(vscode.Uri.file(file), list);
}

export function deactivate(): void {
  core?.dispose();
}
