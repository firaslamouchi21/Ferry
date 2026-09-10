import * as vscode from "vscode";
import { spawn } from "node:child_process";
import net from "node:net";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { writeFrame } from "./framing";

export function bundledDaemonPath(context: vscode.ExtensionContext): string {
  const name = process.platform === "win32" ? "ferry-daemon.exe" : "ferry-daemon";
  return path.join(context.extensionUri.fsPath, "bin", name);
}

export function resolveDaemonBin(context: vscode.ExtensionContext): string {
  const bundled = bundledDaemonPath(context);
  if (fs.existsSync(bundled)) return bundled;
  const configured = vscode.workspace.getConfiguration("ferry").get<string>("daemonPath");
  return configured && configured.length > 0 ? configured : "ferry-daemon";
}

export function daemonReachable(socketPath: string): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = net.connect(socketPath);
    probe.on("connect", () => {
      probe.destroy();
      resolve(true);
    });
    probe.on("error", () => resolve(false));
  });
}

export async function startDaemon(
  context: vscode.ExtensionContext,
  socketPath: string,
): Promise<{ started: boolean; alreadyRunning?: boolean; pid?: number }> {
  if (await daemonReachable(socketPath)) return { started: false, alreadyRunning: true };

  const bin = resolveDaemonBin(context);
  if (path.isAbsolute(bin) && !fs.existsSync(bin)) {
    throw new Error(`ferry-daemon not found at ${bin}`);
  }

  const logPath = path.join(os.tmpdir(), "ferry-vscode-daemon.log");
  const log = fs.openSync(logPath, "a");
  let child;
  try {
    child = spawn(bin, [], { detached: true, stdio: ["ignore", log, log] });
  } catch (err) {
    throw new Error(`could not start ${bin}: ${(err as Error).message ?? err}`);
  }
  child.unref();

  for (let i = 0; i < 50; i += 1) {
    if (await daemonReachable(socketPath)) return { started: true, pid: child.pid };
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`ferry-daemon (pid ${child.pid}) did not accept a connection within 5s — see ${logPath}`);
}

export async function stopDaemon(socketPath: string): Promise<void> {
  if (!(await daemonReachable(socketPath))) return;
  await new Promise<void>((resolve) => {
    const socket = net.connect(socketPath);
    const envelope = { ipc_protocol_version: 1, request_id: `stop-${Date.now()}`, request: { method: "quit" } };
    socket.on("connect", () => writeFrame(socket, Buffer.from(JSON.stringify(envelope))));
    socket.on("error", () => resolve());
    socket.on("close", () => resolve());
    setTimeout(() => {
      socket.destroy();
      resolve();
    }, 2000);
  });
  for (let i = 0; i < 30; i += 1) {
    if (!(await daemonReachable(socketPath))) return;
    await new Promise((r) => setTimeout(r, 100));
  }
}
