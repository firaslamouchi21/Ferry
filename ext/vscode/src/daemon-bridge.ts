import * as vscode from "vscode";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { makeFrameReader, writeFrame } from "./framing";
import { startDaemon } from "./daemon-lifecycle";
import { Subscription } from "./subscription";

export function defaultSocketPath(): string {
  const home = os.homedir();
  if (process.platform === "darwin") {
    return path.join(home, "Library", "Application Support", "dev.ferry.ferry", "ferry.sock");
  }
  if (process.platform === "win32") {
    return `\\\\.\\pipe\\${(process.env.USERNAME ?? "").replace(/[^A-Za-z0-9_-]/g, "_")}-ferry.sock`;
  }
  const base = process.env.XDG_DATA_HOME || path.join(home, ".local", "share");
  return path.join(base, "ferry", "ferry.sock");
}

export function configuredSocketPath(): string {
  const configured = vscode.workspace.getConfiguration("ferry").get<string>("socketPath");
  return configured && configured.length > 0 ? configured : defaultSocketPath();
}

type Outbound = (message: unknown) => void;

interface BridgeRequest {
  kind?: string;
  envelope?: unknown;
  name?: string;
  path?: string;
  content_base64?: string;
}

export class DaemonBridge {
  private disposed = false;
  private subscription: Subscription;

  constructor(
    private postToWebview: Outbound,
    private context: vscode.ExtensionContext,
  ) {
    this.subscription = new Subscription(configuredSocketPath, {
      onConnected: () => this.post({ kind: "phase", phase: "connected" }),
      onDisconnected: () => this.post({ kind: "phase", phase: "disconnected" }),
      onEvent: (event) => this.post({ kind: "event", data: event }),
    });
  }

  private post(message: unknown): void {
    if (!this.disposed) this.postToWebview(message);
  }

  handle(message: BridgeRequest): void {
    switch (message.kind) {
      case "pick-file":
        void this.pickFile();
        return;
      case "pick-save-path":
        void this.pickSavePath(message.name ?? "ferry-item");
        return;
      case "save-file":
        if (message.path && typeof message.content_base64 === "string") {
          void this.saveFile(message.path, message.content_base64);
        }
        return;
      case "start":
        this.post({ kind: "phase", phase: "connecting" });
        this.subscription.start();
        return;
      case "stop":
        this.subscription.stop();
        return;
      case "start-daemon":
        void this.startDaemon();
        return;
      case "request":
        if (message.envelope) this.request(message.envelope as { request_id?: string });
        return;
    }
  }

  private pickResult(uri: vscode.Uri | undefined): void {
    if (uri && uri.scheme !== "file") {
      vscode.window.showWarningMessage("Ferry: choose a file on this computer — the daemon runs here, not on the remote machine.");
      this.post({ kind: "pick-file-result", file: null });
      return;
    }
    this.post({ kind: "pick-file-result", file: uri ? { path: uri.fsPath, name: path.basename(uri.fsPath) } : null });
  }

  private async pickFile(): Promise<void> {
    const picked = await vscode.window.showOpenDialog({ canSelectMany: false, canSelectFolders: false });
    this.pickResult(picked?.[0]);
  }

  private async pickSavePath(name: string): Promise<void> {
    const workspace = vscode.workspace.workspaceFolders?.[0]?.uri;
    const folder = workspace?.scheme === "file" ? workspace : vscode.Uri.file(os.homedir());
    this.pickResult(await vscode.window.showSaveDialog({ defaultUri: vscode.Uri.joinPath(folder, path.basename(name) || "ferry-item") }));
  }

  private async saveFile(target: string, contentBase64: string): Promise<void> {
    try {
      await vscode.workspace.fs.writeFile(vscode.Uri.file(target), Buffer.from(contentBase64, "base64"));
      this.post({ kind: "save-file-result", ok: true });
    } catch (err) {
      this.post({ kind: "save-file-result", ok: false, message: String((err as Error).message ?? err) });
    }
  }

  private async startDaemon(): Promise<void> {
    try {
      await startDaemon(this.context, configuredSocketPath());
      this.post({ kind: "daemon-start-result", ok: true });
      this.subscription.start();
    } catch (err) {
      this.post({ kind: "daemon-start-result", ok: false, message: String((err as Error).message ?? err) });
    }
  }

  private request(envelope: { request_id?: string }): void {
    const socket = net.connect(configuredSocketPath());
    let answered = false;
    const fail = (message: string) => {
      if (answered) return;
      answered = true;
      socket.destroy();
      this.post({ kind: "error", id: envelope.request_id, message });
    };
    const read = makeFrameReader((frame) => {
      if (answered) return;
      const data = JSON.parse(frame.toString());
      answered = true;
      this.post({ kind: "response", data });
      socket.end();
    });
    socket.on("connect", () => writeFrame(socket, Buffer.from(JSON.stringify(envelope))));
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch (err) {
        fail(String((err as Error).message ?? err));
      }
    });
    socket.on("error", (err) => fail(String(err.message ?? err)));
    socket.on("close", () =>
      fail("the daemon closed the connection without answering — it may be older than this extension; run “Ferry: Restart Daemon”"),
    );
  }

  dispose(): void {
    this.disposed = true;
    this.subscription.stop();
  }
}
