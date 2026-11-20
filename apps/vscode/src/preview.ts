// The Mobile Preview panel: the desktop app's preview page (phone body, live screen, input,
// fingerprint sensor, Problems / Logs drawer) in a VS Code web view. The page's API calls come
// here as messages and go to the Core through the bridge; the frames come straight from the Core's
// local preview socket (ws://127.0.0.1, single-use token).
import { randomBytes } from "node:crypto";
import { readdirSync } from "node:fs";
import * as vscode from "vscode";
import { CoreClient } from "./core";

/** Links the page may open (the user guides). */
const DOCS = "https://github.com/Jishnu2608/TUMR/blob/main/docs/user/";

export class PreviewPanel {
  private static current: PreviewPanel | undefined;
  private readonly disposables: vscode.Disposable[] = [];

  static show(context: vscode.ExtensionContext, core: () => CoreClient | undefined): void {
    if (PreviewPanel.current) {
      PreviewPanel.current.panel.reveal(undefined, true);
      return;
    }
    const panel = vscode.window.createWebviewPanel(
      "tumrPreview",
      "Mobile Preview",
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: false },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media", "ui")],
      },
    );
    panel.iconPath = vscode.Uri.joinPath(context.extensionUri, "media", "icon.png");
    PreviewPanel.current = new PreviewPanel(panel, context, core);
    // PLUG-004 (owner decision 2026-11-20): the phone goes to the left of the code, once, when the
    // panel opens; if the user moves it later it stays there
    void PreviewPanel.dockLeft();
  }

  private static async dockLeft(): Promise<void> {
    if (vscode.window.tabGroups.all.length < 2) {
      return;
    }
    try {
      await vscode.commands.executeCommand("workbench.action.moveActiveEditorGroupLeft");
      await vscode.commands.executeCommand("workbench.action.focusNextGroup");
    } catch {
      // the layout stays as VS Code made it
    }
  }

  private constructor(
    private readonly panel: vscode.WebviewPanel,
    context: vscode.ExtensionContext,
    core: () => CoreClient | undefined,
  ) {
    panel.webview.html = this.html(context);
    panel.webview.onDidReceiveMessage(
      async (m: { type?: string; id?: number; method?: string; params?: unknown; url?: string }) => {
        if (m?.type === "rpc" && typeof m.id === "number" && typeof m.method === "string") {
          const c = core();
          if (!c) {
            panel.webview.postMessage({ type: "rpc", id: m.id, error: "tumr.exe was not found" });
            return;
          }
          try {
            const result = await c.call(m.method, m.params ?? {});
            panel.webview.postMessage({ type: "rpc", id: m.id, result });
          } catch (e) {
            panel.webview.postMessage({ type: "rpc", id: m.id, error: String((e as Error).message ?? e) });
          }
        } else if (m?.type === "openDocs" && typeof m.url === "string" && m.url.startsWith(DOCS)) {
          vscode.env.openExternal(vscode.Uri.parse(m.url));
        }
      },
      null,
      this.disposables,
    );
    panel.onDidDispose(() => this.dispose(), null, this.disposables);
  }

  private html(context: vscode.ExtensionContext): string {
    const w = this.panel.webview;
    const dir = vscode.Uri.joinPath(context.extensionUri, "media", "ui");
    let files: string[] = [];
    try {
      files = readdirSync(dir.fsPath);
    } catch {
      // packaged without the UI: the page says so below
    }
    const nonce = randomBytes(16).toString("base64");
    const scripts = files
      .filter((f) => f.endsWith(".js"))
      .map((f) => `<script type="module" nonce="${nonce}" src="${w.asWebviewUri(vscode.Uri.joinPath(dir, f))}"></script>`)
      .join("\n");
    const styles = files
      .filter((f) => f.endsWith(".css"))
      .map((f) => `<link rel="stylesheet" href="${w.asWebviewUri(vscode.Uri.joinPath(dir, f))}">`)
      .join("\n");
    const csp = [
      "default-src 'none'",
      `img-src ${w.cspSource} data:`,
      `style-src ${w.cspSource} 'unsafe-inline'`,
      `font-src ${w.cspSource}`,
      `script-src 'nonce-${nonce}'`,
      // the Core's preview socket, on this PC only
      "connect-src ws://127.0.0.1:*",
    ].join("; ");
    return `<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Mobile Preview</title>
${styles}
<style>html, body, #root { height: 100%; margin: 0; padding: 0; } .pv { height: 100vh; }</style>
</head>
<body>
<div id="root">${files.length ? "" : "The preview page is missing from this build of the extension."}</div>
<script nonce="${nonce}">window.__TUMR_VIEW__ = "preview";</script>
${scripts}
</body>
</html>`;
  }

  private dispose(): void {
    PreviewPanel.current = undefined;
    for (const d of this.disposables) d.dispose();
  }
}
