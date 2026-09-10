import * as vscode from "vscode";
import fs from "node:fs";
import path from "node:path";
import { DaemonBridge, defaultSocketPath } from "./daemon-bridge";
import { daemonReachable, startDaemon, stopDaemon } from "./daemon-lifecycle";
import { PeersTreeProvider } from "./tree";

function socketPath(): string {
  const configured = vscode.workspace.getConfiguration("ferry").get<string>("socketPath");
  return configured && configured.length > 0 ? configured : defaultSocketPath();
}

let panel: vscode.WebviewPanel | undefined;

async function ensureDaemon(context: vscode.ExtensionContext, notifyWhenAlreadyUp: boolean): Promise<void> {
  const sock = socketPath();
  if (await daemonReachable(sock)) {
    if (notifyWhenAlreadyUp) vscode.window.showInformationMessage("Ferry: the daemon is already running.");
    return;
  }
  await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Window, title: "Ferry: starting daemon…" },
    async () => {
      try {
        await startDaemon(context, sock);
        vscode.window.showInformationMessage("Ferry: daemon started.");
      } catch (err) {
        vscode.window.showErrorMessage(
          `Ferry: could not start the daemon — ${String((err as Error).message ?? err)}. Run \`ferry daemon start\` in a terminal.`,
        );
      }
    },
  );
}

export function activate(context: vscode.ExtensionContext) {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  status.text = "$(radio-tower) Ferry";
  status.command = "ferry.open";
  status.show();
  context.subscriptions.push(status);

  const tree = new PeersTreeProvider(socketPath());
  context.subscriptions.push(vscode.window.registerTreeDataProvider("ferryPeers", tree));

  void ensureDaemon(context, false).then(() => tree.refresh());

  context.subscriptions.push(
    vscode.commands.registerCommand("ferry.open", () => openPanel(context)),
    vscode.commands.registerCommand("ferry.pair", () => openPanel(context, "/pair")),
    vscode.commands.registerCommand("ferry.openInbox", () => openPanel(context, "/inbox")),
    vscode.commands.registerCommand("ferry.refreshPeers", () => tree.refresh()),
    vscode.commands.registerCommand("ferry.startDaemon", () => ensureDaemon(context, true)),
    vscode.commands.registerCommand("ferry.restartDaemon", async () => {
      await stopDaemon(socketPath());
      await ensureDaemon(context, false);
      tree.refresh();
    }),
    vscode.commands.registerCommand("ferry.sendFile", async (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (!target) {
        vscode.window.showWarningMessage("Ferry: no file selected.");
        return;
      }
      openPanel(context, `/send?path=${encodeURIComponent(target.fsPath)}`);
    }),
    vscode.commands.registerCommand("ferry.copyFingerprint", async () => {
      const id = await tree.identity();
      if (id) {
        await vscode.env.clipboard.writeText(id.fingerprint);
        vscode.window.showInformationMessage("Ferry: fingerprint copied to clipboard.");
      }
    }),
  );
}

function openPanel(context: vscode.ExtensionContext, route?: string) {
  if (panel) {
    panel.reveal();
    if (route) panel.webview.postMessage({ kind: "navigate", route });
    return;
  }

  panel = vscode.window.createWebviewPanel("ferryPanel", "Ferry", vscode.ViewColumn.Active, {
    enableScripts: true,
    retainContextWhenHidden: true,
    localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media", "ui")],
  });

  const bridge = new DaemonBridge(socketPath(), (message) => panel?.webview.postMessage(message), context);
  panel.webview.onDidReceiveMessage((message) => bridge.handle(message));
  panel.webview.html = renderHtml(context, panel.webview);

  panel.onDidDispose(() => {
    bridge.dispose();
    panel = undefined;
  });
}

function renderHtml(context: vscode.ExtensionContext, webview: vscode.Webview): string {
  const uiDir = vscode.Uri.joinPath(context.extensionUri, "media", "ui");
  const indexPath = path.join(uiDir.fsPath, "index.html");
  let html = fs.readFileSync(indexPath, "utf8");
  const nonce = String(Math.random()).slice(2);

  html = html.replace(/(src|href)="\/([^"]+)"/g, (_m, attr, rel) => {
    const uri = webview.asWebviewUri(vscode.Uri.joinPath(uiDir, rel));
    return `${attr}="${uri}"`;
  });

  const csp = [
    `default-src 'none'`,
    `img-src ${webview.cspSource} data:`,
    `style-src ${webview.cspSource} 'unsafe-inline'`,
    `font-src ${webview.cspSource}`,
    `script-src ${webview.cspSource} 'nonce-${nonce}'`,
    `connect-src ${webview.cspSource}`,
  ].join("; ");

  html = html
    .replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`)
    .replace(/<script type="module"/g, `<script type="module" nonce="${nonce}"`);

  return html;
}

export function deactivate() {
  panel?.dispose();
}
