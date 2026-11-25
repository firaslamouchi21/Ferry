import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { configuredSocketPath, DaemonBridge } from "./daemon-bridge";
import { daemonReachable, runningDaemonIsOutdated, startDaemon, stopDaemon } from "./daemon-lifecycle";
import { identity } from "./identity";
import { DaemonMonitor } from "./monitor";

let panel: vscode.WebviewPanel | undefined;
const webviewMessages = new vscode.EventEmitter<unknown>();

export interface FerryExtensionApi {
  onWebviewMessage: vscode.Event<unknown>;
}

function autoStartEnabled(): boolean {
  return vscode.workspace.getConfiguration("ferry").get<boolean>("autoStartDaemon", true);
}

async function ensureDaemon(context: vscode.ExtensionContext, announce: boolean): Promise<boolean> {
  const socket = configuredSocketPath();
  if (await daemonReachable(socket)) {
    if (announce) vscode.window.showInformationMessage("Ferry: the daemon is already running.");
    return true;
  }
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Window, title: "Ferry: starting daemon…" },
    async () => {
      try {
        await startDaemon(context, socket);
        if (announce) vscode.window.showInformationMessage("Ferry: daemon started.");
        return true;
      } catch (err) {
        const reason = String((err as Error).message ?? err);
        if (announce) vscode.window.showErrorMessage(`Ferry: could not start the daemon — ${reason}`);
        else console.warn(`Ferry: could not start the daemon — ${reason}`);
        return false;
      }
    },
  );
}

async function restartDaemon(context: vscode.ExtensionContext): Promise<void> {
  if (!(await stopDaemon(configuredSocketPath()))) {
    vscode.window.showErrorMessage("Ferry: the running daemon did not shut down, so it was not restarted.");
    return;
  }
  await ensureDaemon(context, true);
}

let offeredUpgrade = false;

async function offerUpgrade(context: vscode.ExtensionContext): Promise<void> {
  if (offeredUpgrade || !(await runningDaemonIsOutdated(context, configuredSocketPath()))) return;
  offeredUpgrade = true;
  const choice = await vscode.window.showWarningMessage(
    "Ferry: the running daemon is older than this extension. Restart it to use the bundled version?",
    "Restart Daemon",
  );
  if (choice === "Restart Daemon") await restartDaemon(context);
}

function wireWebview(context: vscode.ExtensionContext, webview: vscode.Webview, initialRoute?: string): DaemonBridge {
  webview.options = {
    enableScripts: true,
    localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media", "ui")],
  };
  const bridge = new DaemonBridge((message) => void webview.postMessage(message), context);
  let pendingRoute = initialRoute;
  webview.onDidReceiveMessage((message: { kind?: string }) => {
    webviewMessages.fire(message);
    if (message?.kind === "start" && pendingRoute) {
      void webview.postMessage({ kind: "navigate", route: pendingRoute });
      pendingRoute = undefined;
    }
    bridge.handle(message);
  });
  webview.html = renderHtml(context, webview);
  return bridge;
}

class SidebarProvider implements vscode.WebviewViewProvider {
  constructor(private context: vscode.ExtensionContext) {}

  resolveWebviewView(view: vscode.WebviewView): void {
    const bridge = wireWebview(this.context, view.webview);
    view.onDidDispose(() => bridge.dispose());
    if (autoStartEnabled()) void ensureDaemon(this.context, false);
  }
}

export function activate(context: vscode.ExtensionContext): FerryExtensionApi {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  status.name = "Ferry";
  status.command = "ferry.open";
  status.show();

  const monitor = new DaemonMonitor(
    status,
    (route) => openPanel(context, route),
    () => void offerUpgrade(context),
  );
  monitor.start();

  context.subscriptions.push(
    webviewMessages,
    status,
    monitor,
    vscode.window.registerWebviewViewProvider("ferryMain", new SidebarProvider(context), {
      webviewOptions: { retainContextWhenHidden: true },
    }),
    vscode.commands.registerCommand("ferry.open", () => openPanel(context)),
    vscode.commands.registerCommand("ferry.pair", () => openPanel(context, "/pair")),
    vscode.commands.registerCommand("ferry.openInbox", () => openPanel(context, "/inbox")),
    vscode.commands.registerCommand("ferry.startDaemon", () => ensureDaemon(context, true)),
    vscode.commands.registerCommand("ferry.stopDaemon", async () => {
      if (await stopDaemon(configuredSocketPath())) vscode.window.showInformationMessage("Ferry: the daemon is stopped.");
      else vscode.window.showErrorMessage("Ferry: the daemon did not shut down.");
    }),
    vscode.commands.registerCommand("ferry.restartDaemon", () => restartDaemon(context)),
    vscode.commands.registerCommand("ferry.sendFile", (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (!target) {
        vscode.window.showWarningMessage("Ferry: no file selected.");
        return;
      }
      if (target.scheme !== "file") {
        vscode.window.showWarningMessage("Ferry: only files saved on this computer can be sent.");
        return;
      }
      openPanel(context, `/send?path=${encodeURIComponent(target.fsPath)}`);
    }),
    vscode.commands.registerCommand("ferry.copyFingerprint", async () => {
      const id = await identity(configuredSocketPath());
      if (id) {
        await vscode.env.clipboard.writeText(id.fingerprint);
        vscode.window.showInformationMessage("Ferry: fingerprint copied to clipboard.");
      } else {
        vscode.window.showWarningMessage("Ferry: the daemon isn't reachable, so there is no fingerprint to copy yet.");
      }
    }),
  );

  if (autoStartEnabled()) void ensureDaemon(context, false);

  return { onWebviewMessage: webviewMessages.event };
}

function openPanel(context: vscode.ExtensionContext, route?: string) {
  if (panel) {
    panel.reveal();
    if (route) void panel.webview.postMessage({ kind: "navigate", route });
    return;
  }

  panel = vscode.window.createWebviewPanel("ferryPanel", "Ferry", vscode.ViewColumn.Active, {
    enableScripts: true,
    retainContextWhenHidden: true,
    localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "media", "ui")],
  });
  panel.iconPath = vscode.Uri.joinPath(context.extensionUri, "media", "icon.png");

  const bridge = wireWebview(context, panel.webview, route);
  panel.onDidDispose(() => {
    bridge.dispose();
    panel = undefined;
  });
  if (autoStartEnabled()) void ensureDaemon(context, false);
}

function renderHtml(context: vscode.ExtensionContext, webview: vscode.Webview): string {
  const uiDir = vscode.Uri.joinPath(context.extensionUri, "media", "ui");
  let html = fs.readFileSync(path.join(uiDir.fsPath, "index.html"), "utf8");
  const nonce = randomBytes(16).toString("base64url");

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

  return html
    .replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`)
    .replace(/<script type="module"/g, `<script type="module" nonce="${nonce}"`);
}

export function deactivate() {
  panel?.dispose();
}
