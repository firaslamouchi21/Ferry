import { spawn } from "node:child_process";
import net from "node:net";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { WebSocketServer } from "ws";

const MAX_FRAME_BYTES = 64 * 1024 * 1024;
const HERE = path.dirname(fileURLToPath(import.meta.url));

export function defaultSocketPath() {
  if (process.env.FERRY_SOCK) return process.env.FERRY_SOCK;
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

function daemonBinary() {
  const name = process.platform === "win32" ? "ferry-daemon.exe" : "ferry-daemon";
  const candidates = [
    process.env.FERRY_DAEMON_BIN,
    path.join(HERE, "..", "..", "target", "debug", name),
    path.join(HERE, "..", "..", "target", "release", name),
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }
  return name;
}

function socketReachable(socketPath) {
  return new Promise((resolve) => {
    const probe = net.connect(socketPath);
    probe.on("connect", () => {
      probe.destroy();
      resolve(true);
    });
    probe.on("error", () => resolve(false));
  });
}

async function startDaemon(socketPath) {
  if (await socketReachable(socketPath)) return { started: false, alreadyRunning: true };
  const bin = daemonBinary();
  const logPath = path.join(os.tmpdir(), "ferry-dev-daemon.log");
  const log = fs.openSync(logPath, "a");
  let child;
  try {
    child = spawn(bin, [], { detached: true, stdio: ["ignore", log, log] });
  } catch (err) {
    throw new Error(`could not spawn ${bin}: ${err.message ?? err}`);
  }
  child.unref();
  for (let i = 0; i < 50; i += 1) {
    if (await socketReachable(socketPath)) return { started: true, pid: child.pid, logPath };
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`ferry-daemon (pid ${child.pid}) did not accept a connection within 5s — see ${logPath}`);
}

function frameReader(onFrame) {
  let buf = Buffer.alloc(0);
  return (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    while (buf.length >= 4) {
      const len = buf.readUInt32BE(0);
      if (len > MAX_FRAME_BYTES) {
        throw new Error(`oversize frame declared: ${len}`);
      }
      if (buf.length < 4 + len) return;
      const payload = buf.subarray(4, 4 + len);
      buf = buf.subarray(4 + len);
      onFrame(payload);
    }
  };
}

function writeFrame(socket, payloadBuf) {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payloadBuf.length, 0);
  socket.write(Buffer.concat([header, payloadBuf]));
}

function handleConnection(ws, socketPath) {
  const pending = new Map();
  let subSocket = null;

  ws.on("message", (raw) => {
    let msg;
    try {
      msg = JSON.parse(raw.toString());
    } catch {
      return;
    }
    const envelope = msg.envelope ?? msg;
    const isSubscribe = envelope?.request?.method === "subscribe";
    const payload = Buffer.from(JSON.stringify(envelope));
    if (payload.length > MAX_FRAME_BYTES) {
      ws.send(JSON.stringify({ kind: "error", id: envelope?.request_id, message: "request exceeds frame cap" }));
      return;
    }

    const socket = net.connect(socketPath);
    socket.on("error", (err) => {
      ws.send(JSON.stringify({ kind: "error", id: envelope?.request_id, message: String(err.message ?? err) }));
    });
    socket.on("connect", () => writeFrame(socket, payload));

    if (isSubscribe) {
      subSocket = socket;
      const read = frameReader((frame) => {
        ws.send(JSON.stringify({ kind: "event", data: JSON.parse(frame.toString()) }));
      });
      socket.on("data", (c) => {
        try {
          read(c);
        } catch (err) {
          ws.send(JSON.stringify({ kind: "error", message: String(err.message ?? err) }));
          socket.destroy();
        }
      });
      socket.on("close", () => ws.send(JSON.stringify({ kind: "subscribe_closed" })));
    } else {
      const read = frameReader((frame) => {
        ws.send(JSON.stringify({ kind: "response", data: JSON.parse(frame.toString()) }));
        socket.end();
      });
      socket.on("data", (c) => {
        try {
          read(c);
        } catch (err) {
          ws.send(JSON.stringify({ kind: "error", id: envelope?.request_id, message: String(err.message ?? err) }));
          socket.destroy();
        }
      });
    }
    pending.set(envelope?.request_id, socket);
  });

  ws.on("close", () => {
    if (subSocket) subSocket.destroy();
    for (const socket of pending.values()) socket.destroy();
    pending.clear();
  });
}

export function attachBridge(server, opts = {}) {
  const httpServer = server?.httpServer ?? server;
  const middlewares = server?.middlewares;
  const socketPath = opts.socketPath ?? defaultSocketPath();
  const wss = new WebSocketServer({ noServer: true });
  wss.on("connection", (ws) => handleConnection(ws, socketPath));
  wss.on("error", () => {});
  httpServer.on("upgrade", (req, socket, head) => {
    let pathname;
    try {
      pathname = new URL(req.url ?? "/", "http://localhost").pathname;
    } catch {
      return;
    }
    if (pathname !== "/ferry-ipc") return;
    socketReachable(socketPath).then((reachable) => {
      if (!reachable) {
        socket.destroy();
        return;
      }
      wss.handleUpgrade(req, socket, head, (ws) => wss.emit("connection", ws, req));
    });
  });

  if (middlewares) {
    middlewares.use("/ferry-daemon/start", (req, res, next) => {
      if (req.method !== "POST") return next();
      startDaemon(socketPath)
        .then((info) => {
          res.statusCode = 200;
          res.setHeader("content-type", "application/json");
          res.end(JSON.stringify(info));
        })
        .catch((err) => {
          res.statusCode = 500;
          res.end(String(err.message ?? err));
        });
    });
  }

  return wss;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const port = Number(process.env.BRIDGE_PORT ?? 8787);
  const wss = new WebSocketServer({ port, path: "/ferry-ipc" });
  const socketPath = defaultSocketPath();
  wss.on("connection", (ws) => handleConnection(ws, socketPath));
  console.log(`ferry dev-bridge: ws://localhost:${port}/ferry-ipc -> ${socketPath}`);
}
