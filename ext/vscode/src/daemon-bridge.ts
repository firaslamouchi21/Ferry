import * as vscode from "vscode";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { makeFrameReader, writeFrame } from "./framing";
import { startDaemon } from "./daemon-lifecycle";

export function defaultSocketPath(): string {
  const home = os.homedir();
  if (process.platform === "darwin") {
    return path.join(home, "Library", "Application Support", "dev.ferry.ferry", "ferry.sock");
  }
  if (process.platform === "win32") {
    return "\\\\.\\pipe\\ferry.sock";
  }
  const base = process.env.XDG_DATA_HOME ?? path.join(home, ".local", "share");
  return path.join(base, "ferry", "ferry.sock");
}

type Outbound = (message: unknown) => void;

export class DaemonBridge {
  private subscribeSocket: net.Socket | null = null;

  constructor(
    private socketPath: string,
    private post: Outbound,
    private context: vscode.ExtensionContext,
  ) {}

  handle(message: { kind?: string; envelope?: unknown }): void {
    if (message.kind === "start") {
      this.openSubscription();
      this.post({ kind: "phase", phase: "connecting" });
      return;
    }
    if (message.kind === "stop") {
      this.subscribeSocket?.destroy();
      this.subscribeSocket = null;
      return;
    }
    if (message.kind === "start-daemon") {
      void this.startDaemon();
      return;
    }
    if (message.kind === "request" && message.envelope) {
      this.request(message.envelope as { request_id?: string });
    }
  }

  private async startDaemon(): Promise<void> {
    try {
      await startDaemon(this.context, this.socketPath);
      this.post({ kind: "daemon-start-result", ok: true });
      this.openSubscription();
    } catch (err) {
      this.post({ kind: "daemon-start-result", ok: false, message: String((err as Error).message ?? err) });
    }
  }

  private request(envelope: { request_id?: string }): void {
    const socket = net.connect(this.socketPath);
    const payload = Buffer.from(JSON.stringify(envelope));
    socket.on("error", (err) =>
      this.post({ kind: "error", id: envelope.request_id, message: String(err.message ?? err) }),
    );
    socket.on("connect", () => writeFrame(socket, payload));
    const read = makeFrameReader((frame) => {
      this.post({ kind: "response", data: JSON.parse(frame.toString()) });
      socket.end();
    });
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch (err) {
        this.post({ kind: "error", id: envelope.request_id, message: String((err as Error).message) });
        socket.destroy();
      }
    });
  }

  openSubscription(): void {
    this.subscribeSocket?.destroy();
    const socket = net.connect(this.socketPath);
    this.subscribeSocket = socket;
    const envelope = {
      ipc_protocol_version: 1,
      request_id: `sub-${Date.now()}`,
      request: { method: "subscribe" },
    };
    socket.on("connect", () => {
      writeFrame(socket, Buffer.from(JSON.stringify(envelope)));
      this.post({ kind: "phase", phase: "connected" });
    });
    const read = makeFrameReader((frame) => {
      this.post({ kind: "event", data: JSON.parse(frame.toString()) });
    });
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch {
        socket.destroy();
      }
    });
    socket.on("close", () => {
      this.post({ kind: "phase", phase: "disconnected" });
      if (this.subscribeSocket === socket) {
        this.subscribeSocket = null;
        setTimeout(() => this.openSubscription(), 1500);
      }
    });
    socket.on("error", () => socket.destroy());
  }

  dispose(): void {
    this.subscribeSocket?.destroy();
    this.subscribeSocket = null;
  }
}
