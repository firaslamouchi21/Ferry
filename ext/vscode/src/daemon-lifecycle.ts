import * as vscode from "vscode";
import { spawn } from "node:child_process";
import net from "node:net";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { callDaemon } from "./framing";
import { defaultSocketPath } from "./daemon-bridge";

const DAEMON_NAME = process.platform === "win32" ? "ferry-daemon.exe" : "ferry-daemon";
const START_TIMEOUT_MS = 10_000;
const MAX_LOG_BYTES = 5 * 1024 * 1024;

export function extensionVersion(context: vscode.ExtensionContext): string {
  return String(context.extension?.packageJSON?.version ?? "0.0.0");
}

export function bundledDaemonPath(context: vscode.ExtensionContext): string {
  const bin = path.join(context.extensionUri.fsPath, "bin");
  const platforms = [`${process.platform}-${process.arch}`];
  if (process.platform === "win32") platforms.push("win32-x64");
  const candidates = [...platforms.map((platform) => path.join(bin, platform, DAEMON_NAME)), path.join(bin, DAEMON_NAME)];
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? path.join(bin, DAEMON_NAME);
}

function installBundledDaemon(context: vscode.ExtensionContext, bundled: string): string {
  const root = path.join(context.globalStorageUri.fsPath, "daemon");
  const target = path.join(root, extensionVersion(context), DAEMON_NAME);
  const source = fs.statSync(bundled);
  const installed = fs.existsSync(target) ? fs.statSync(target) : undefined;
  if (!installed || installed.size !== source.size || installed.mtimeMs < source.mtimeMs) {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    const partial = `${target}.${process.pid}.part`;
    fs.copyFileSync(bundled, partial);
    if (process.platform !== "win32") fs.chmodSync(partial, 0o755);
    try {
      fs.renameSync(partial, target);
    } catch (err) {
      fs.rmSync(partial, { force: true });
      if (!installed) throw err;
    }
  }
  for (const name of fs.readdirSync(root)) {
    if (name === extensionVersion(context)) continue;
    try {
      fs.rmSync(path.join(root, name), { recursive: true, force: true });
    } catch {
      continue;
    }
  }
  return target;
}

export function resolveDaemonBin(context: vscode.ExtensionContext): string {
  const bundled = bundledDaemonPath(context);
  if (fs.existsSync(bundled)) return installBundledDaemon(context, bundled);
  const configured = vscode.workspace.getConfiguration("ferry").get<string>("daemonPath");
  return configured && configured.length > 0 ? configured : "ferry-daemon";
}

export function daemonReachable(socketPath: string): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = net.connect(socketPath);
    const settle = (reachable: boolean) => {
      clearTimeout(timer);
      probe.destroy();
      resolve(reachable);
    };
    const timer = setTimeout(() => settle(false), 2000);
    probe.on("connect", () => settle(true));
    probe.on("error", () => settle(false));
  });
}

export type StartResult = { started: boolean; pid?: number };

let starting: Promise<StartResult> | undefined;

export function startDaemon(context: vscode.ExtensionContext, socketPath: string): Promise<StartResult> {
  starting ??= launch(context, socketPath).finally(() => {
    starting = undefined;
  });
  return starting;
}

function openLog(context: vscode.ExtensionContext): { fd: number; logPath: string } {
  fs.mkdirSync(context.globalStorageUri.fsPath, { recursive: true });
  const logPath = path.join(context.globalStorageUri.fsPath, "daemon.log");
  const oversized = fs.existsSync(logPath) && fs.statSync(logPath).size > MAX_LOG_BYTES;
  return { fd: fs.openSync(logPath, oversized ? "w" : "a", 0o600), logPath };
}

function lastLogLine(logPath: string): string {
  try {
    return fs.readFileSync(logPath, "utf8").trim().split("\n").pop() ?? "";
  } catch {
    return "";
  }
}

async function launch(context: vscode.ExtensionContext, socketPath: string): Promise<StartResult> {
  if (await daemonReachable(socketPath)) return { started: false };
  if (socketPath !== defaultSocketPath()) {
    throw new Error(`ferry.socketPath points at ${socketPath}, which this extension does not manage — start that daemon yourself`);
  }

  const bin = resolveDaemonBin(context);
  if (path.isAbsolute(bin) && !fs.existsSync(bin)) throw new Error(`ferry-daemon not found at ${bin}`);

  const { fd, logPath } = openLog(context);
  let failure: string | undefined;
  let pid: number | undefined;
  try {
    const child = spawn(bin, [], {
      detached: true,
      windowsHide: true,
      cwd: os.homedir(),
      stdio: ["ignore", fd, fd],
    });
    pid = child.pid;
    child.on("error", (err) => {
      failure = err.message;
    });
    child.on("exit", (code) => {
      failure ??= lastLogLine(logPath) || `it exited with code ${code}`;
    });
    child.unref();
  } finally {
    fs.closeSync(fd);
  }

  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (await daemonReachable(socketPath)) return { started: true, pid };
    if (failure) throw new Error(`could not start ${path.basename(bin)}: ${failure}`);
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`ferry-daemon did not accept a connection within ${START_TIMEOUT_MS / 1000}s — see ${logPath}`);
}

export async function stopDaemon(socketPath: string): Promise<boolean> {
  if (!(await daemonReachable(socketPath))) return true;
  await callDaemon(socketPath, { method: "quit" }, 2000).catch(() => undefined);
  for (let i = 0; i < 50; i += 1) {
    if (!(await daemonReachable(socketPath))) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
}

export function isOlder(version: string, than: string): boolean {
  const parse = (v: string) => v.split("-")[0].split(".").map((part) => Number.parseInt(part, 10) || 0);
  const [a, b] = [parse(version), parse(than)];
  for (let i = 0; i < 3; i += 1) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) < (b[i] ?? 0);
  }
  return false;
}

export async function runningDaemonIsOutdated(context: vscode.ExtensionContext, socketPath: string): Promise<boolean> {
  if (!fs.existsSync(bundledDaemonPath(context))) return false;
  try {
    const status = await callDaemon<{ daemon_version?: string }>(socketPath, { method: "status" });
    return !status.daemon_version || isOlder(status.daemon_version, extensionVersion(context));
  } catch (err) {
    return /protocol/i.test(String((err as Error).message));
  }
}
