// VSC-001: Toolchain Universal Mobile Runtime for VS Code.
//
// * Run on Emulator: `tumr run --no-preview <folder>` as a task (its output and the trust question
//   in VS Code's terminal), then the Mobile Preview panel beside the editor.
// * Reload / Restart / Rebuild / Stop and the status bar: Core API calls through the bridge.
// * Problems: the run's build errors, runtime errors and crashes in the Problems panel.
// * Connect Copilot: `tumr agent setup vscode` (MCP tools + instructions for agent mode).
// Saved files reload by themselves: the Runtime Core watches the project.
import * as path from "node:path";
import * as vscode from "vscode";
import { CoreClient } from "./core";
import { findTumr } from "./locate";
import { PreviewPanel } from "./preview";
import { place, RuntimeProblem } from "./problems";

let core: CoreClient | undefined;
let coreExe = "";

function tumrExe(): string | undefined {
  const setting = vscode.workspace.getConfiguration("tumr").get<string>("path") ?? "";
  const folders = (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath);
  return findTumr(setting, process.env, folders);
}

function getCore(): CoreClient | undefined {
  const exe = tumrExe();
  if (!exe) return undefined;
  if (!core || exe !== coreExe) {
    core?.dispose();
    core = new CoreClient(exe);
    coreExe = exe;
  }
  return core;
}

async function requireTumr(): Promise<string | undefined> {
  const exe = tumrExe();
  if (exe) return exe;
  const pick = await vscode.window.showErrorMessage(
    "Toolchain Universal Mobile Runtime: tumr.exe was not found (not on PATH, not in the default install folder).",
    "Locate tumr.exe",
  );
  if (pick) return locate();
  return undefined;
}

async function locate(): Promise<string | undefined> {
  const r = await vscode.window.showOpenDialog({
    canSelectMany: false,
    openLabel: "Use this tumr.exe",
    filters: { Programs: ["exe"] },
  });
  if (!r?.length) return undefined;
  await vscode.workspace.getConfiguration("tumr").update("path", r[0].fsPath, vscode.ConfigurationTarget.Global);
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

/** Runs `tumr <args>` as a task in VS Code's terminal (interactive: trust and agent questions). */
async function tumrTask(name: string, args: string[], folder: string): Promise<void> {
  const exe = await requireTumr();
  if (!exe) return;
  const task = new vscode.Task(
    { type: "tumr", task: name },
    vscode.TaskScope.Workspace,
    name,
    "Toolchain Universal Mobile Runtime",
    new vscode.ProcessExecution(exe, args, { cwd: folder }),
  );
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated, clear: true };
  await vscode.tasks.executeTask(task);
}

async function coreCall(label: string, method: string, params: unknown): Promise<any> {
  const c = getCore();
  if (!c) {
    await requireTumr();
    return undefined;
  }
  try {
    return await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Toolchain Universal Mobile Runtime: ${label}…` },
      () => c.call(method, params),
    );
  } catch (e) {
    vscode.window.showErrorMessage(`Toolchain Universal Mobile Runtime: ${label} failed: ${(e as Error).message}`);
    return undefined;
  }
}

async function reload(kind: "hot" | "restart" | "full", label: string): Promise<void> {
  const r = await coreCall(label, "project.reload", { kind });
  if (!r) return;
  if (r.status === "unsupported") {
    vscode.window.showInformationMessage(`Toolchain Universal Mobile Runtime: ${r.message ?? "this app does not support that reload"}`);
  } else if (r.status !== "reloaded") {
    vscode.window.showWarningMessage(`Toolchain Universal Mobile Runtime: ${label}: ${r.error ?? r.status} — see the Problems panel.`);
  } else {
    vscode.window.setStatusBarMessage(`$(check) ${label} in ${r.ms} ms`, 4000);
  }
}

export function activate(context: vscode.ExtensionContext): void {
  vscode.commands.executeCommand("setContext", "tumr.active", true);
  const diagnostics = vscode.languages.createDiagnosticCollection("tumr");
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = "tumr.menu";
  status.text = "$(device-mobile) Mobile";
  status.tooltip = "Toolchain Universal Mobile Runtime";
  status.show();
  context.subscriptions.push(diagnostics, status, { dispose: () => core?.dispose() });

  const cmd = (id: string, f: () => unknown) => context.subscriptions.push(vscode.commands.registerCommand(id, f));

  cmd("tumr.run", async () => {
    const folder = await pickFolder();
    if (!folder) return;
    await tumrTask("Run on Emulator", ["run", "--no-preview", folder], folder);
    if (vscode.workspace.getConfiguration("tumr").get<boolean>("openPreviewOnRun", true)) {
      PreviewPanel.show(context, getCore);
    }
  });
  cmd("tumr.reload", () => reload("hot", "Reload"));
  cmd("tumr.restart", () => reload("restart", "Restart"));
  cmd("tumr.rebuild", () => reload("full", "Rebuild"));
  cmd("tumr.stop", async () => {
    if (await coreCall("Stop", "project.stop", {})) vscode.window.setStatusBarMessage("$(debug-stop) App stopped", 3000);
  });
  cmd("tumr.openPreview", async () => {
    if (await requireTumr()) PreviewPanel.show(context, getCore);
  });
  cmd("tumr.connectCopilot", async () => {
    const folder = await pickFolder();
    if (folder) await tumrTask("Connect Copilot", ["agent", "setup", "vscode", "--project", folder], folder);
  });
  cmd("tumr.locate", locate);
  cmd("tumr.menu", async () => {
    const items: (vscode.QuickPickItem & { id: string })[] = [
      { id: "tumr.run", label: "$(play) Run on Emulator" },
      { id: "tumr.openPreview", label: "$(device-mobile) Open Mobile Preview" },
      { id: "tumr.reload", label: "$(refresh) Reload (hot)" },
      { id: "tumr.restart", label: "$(debug-restart) Restart the App" },
      { id: "tumr.rebuild", label: "$(tools) Rebuild and Reinstall" },
      { id: "tumr.stop", label: "$(debug-stop) Stop the App" },
      { id: "tumr.connectCopilot", label: "$(copilot) Connect Copilot (agent mode)" },
    ];
    const pick = await vscode.window.showQuickPick(items, { placeHolder: "Toolchain Universal Mobile Runtime" });
    if (pick) vscode.commands.executeCommand(pick.id);
  });

  // status bar and Problems: what runs, every 2 s while VS Code is focused
  let busy = false;
  const refresh = async () => {
    if (busy || !vscode.window.state.focused) return;
    const c = getCore();
    if (!c) {
      status.text = "$(device-mobile) Mobile: tumr.exe not found";
      return;
    }
    busy = true;
    try {
      const s = await c.call("project.status", {}, 15_000);
      const run = s?.run;
      const mine = run && (vscode.workspace.workspaceFolders ?? []).some((f) => samePath(f.uri.fsPath, run.project));
      vscode.commands.executeCommand("setContext", "tumr.running", !!mine);
      if (!run) {
        status.text = "$(device-mobile) Mobile: not running";
        status.tooltip = "Toolchain Universal Mobile Runtime — nothing runs. Click for actions.";
      } else {
        const alive = s.alive === false ? " (stopped)" : "";
        status.text = `$(device-mobile) ${run.package}${alive}`;
        status.tooltip = `Toolchain Universal Mobile Runtime — ${run.adapter} app on ${run.serial}. Click for actions.`;
      }
      if (vscode.workspace.getConfiguration("tumr").get<boolean>("problems", true) && mine) {
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
