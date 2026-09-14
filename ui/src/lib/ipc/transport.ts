import type { IpcEnvelope, IpcEvent, IpcResponse } from "./types";

export type ConnectionPhase = "connecting" | "connected" | "disconnected";

export interface Transport {
  send(envelope: IpcEnvelope): Promise<IpcResponse>;
  onEvent(listener: (event: IpcEvent) => void): () => void;
  onPhase(listener: (phase: ConnectionPhase) => void): () => void;
  phase(): ConnectionPhase;
  start(): void;
  stop(): void;
  retryNow(): void;
  canStartDaemon(): boolean;
  startDaemon(): Promise<void>;
}

type BridgeMessage =
  | { kind: "response"; data: IpcResponse }
  | { kind: "event"; data: IpcEvent }
  | { kind: "error"; id?: string; message: string }
  | { kind: "daemon-start-result"; ok: boolean; message?: string }
  | { kind: "subscribe_closed" };

class Emitter<T> {
  private listeners = new Set<(value: T) => void>();
  add(listener: (value: T) => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  emit(value: T) {
    for (const listener of this.listeners) listener(value);
  }
}

export class WebSocketTransport implements Transport {
  private ws: WebSocket | null = null;
  private events = new Emitter<IpcEvent>();
  private phases = new Emitter<ConnectionPhase>();
  private currentPhase: ConnectionPhase = "disconnected";
  private pending = new Map<string, (response: IpcResponse) => void>();
  private rejecters = new Map<string, (error: Error) => void>();
  private reconnectDelay = 500;
  private stopped = false;

  constructor(private url: string) {}

  phase() {
    return this.currentPhase;
  }

  private setPhase(phase: ConnectionPhase) {
    if (this.currentPhase === phase) return;
    this.currentPhase = phase;
    this.phases.emit(phase);
  }

  start() {
    this.stopped = false;
    this.connect();
  }

  stop() {
    this.stopped = true;
    this.teardown(this.ws);
    this.ws = null;
    this.setPhase("disconnected");
  }

  retryNow() {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) return;
    this.teardown(this.ws);
    this.ws = null;
    this.reconnectDelay = 500;
    this.stopped = false;
    this.connect();
  }

  canStartDaemon() {
    return typeof fetch === "function" && typeof window !== "undefined";
  }

  async startDaemon() {
    const base = typeof window !== "undefined" ? window.location.origin : "";
    const res = await fetch(`${base}/ferry-daemon/start`, { method: "POST" });
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(body || `the dev bridge could not start the daemon (HTTP ${res.status})`);
    }
    this.retryNow();
  }

  private teardown(ws: WebSocket | null) {
    if (!ws) return;
    ws.onopen = null;
    ws.onclose = null;
    ws.onerror = null;
    ws.onmessage = null;
    try {
      if (ws.readyState === 0) {
        ws.addEventListener("open", () => ws.close());
      } else {
        ws.close();
      }
    } catch {
      this.ws = null;
    }
  }

  private connect() {
    if (this.stopped) return;
    this.setPhase("connecting");
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch (err) {
      console.error(`ferry: could not open ${this.url}: ${String((err as Error).message ?? err)}`);
      this.setPhase("disconnected");
      if (!this.stopped) {
        setTimeout(() => this.connect(), this.reconnectDelay);
        this.reconnectDelay = Math.min(this.reconnectDelay * 2, 8000);
      }
      return;
    }
    this.ws = ws;

    ws.onopen = () => {
      if (this.ws !== ws) return;
      this.reconnectDelay = 500;
      this.setPhase("connected");
      this.openSubscription();
    };

    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.failAllPending(new Error("bridge connection closed"));
      this.setPhase("disconnected");
      if (!this.stopped) {
        setTimeout(() => this.connect(), this.reconnectDelay);
        this.reconnectDelay = Math.min(this.reconnectDelay * 2, 8000);
      }
    };

    ws.onerror = () => ws.close();

    ws.onmessage = (raw) => {
      if (this.ws !== ws) return;
      let msg: BridgeMessage;
      try {
        msg = JSON.parse(typeof raw.data === "string" ? raw.data : "");
      } catch {
        return;
      }
      if (msg.kind === "event") {
        this.events.emit(msg.data);
      } else if (msg.kind === "response") {
        const id = msg.data.request_id;
        this.pending.get(id)?.(msg.data);
        this.pending.delete(id);
        this.rejecters.delete(id);
      } else if (msg.kind === "error") {
        if (msg.id && this.rejecters.has(msg.id)) {
          this.rejecters.get(msg.id)?.(new Error(msg.message));
          this.pending.delete(msg.id);
          this.rejecters.delete(msg.id);
        }
      }
    };
  }

  private openSubscription() {
    const envelope: IpcEnvelope = {
      ipc_protocol_version: 1,
      request_id: `sub-${Date.now()}`,
      request: { method: "subscribe" },
    };
    this.ws?.send(JSON.stringify({ envelope }));
  }

  private failAllPending(error: Error) {
    for (const reject of this.rejecters.values()) reject(error);
    this.pending.clear();
    this.rejecters.clear();
  }

  send(envelope: IpcEnvelope): Promise<IpcResponse> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error("daemon transport not connected"));
        return;
      }
      this.pending.set(envelope.request_id, resolve);
      this.rejecters.set(envelope.request_id, reject);
      this.ws.send(JSON.stringify({ envelope }));
      setTimeout(() => {
        if (this.rejecters.has(envelope.request_id)) {
          this.rejecters.get(envelope.request_id)?.(new Error("daemon request timed out"));
          this.pending.delete(envelope.request_id);
          this.rejecters.delete(envelope.request_id);
        }
      }, 15000);
    });
  }

  onEvent(listener: (event: IpcEvent) => void) {
    return this.events.add(listener);
  }

  onPhase(listener: (phase: ConnectionPhase) => void) {
    return this.phases.add(listener);
  }
}

interface VsCodeApi {
  postMessage(message: unknown): void;
}

declare global {
  interface Window {
    acquireVsCodeApi?: () => VsCodeApi;
  }
}

export class VsCodeTransport implements Transport {
  private api: VsCodeApi;
  private events = new Emitter<IpcEvent>();
  private phases = new Emitter<ConnectionPhase>();
  private currentPhase: ConnectionPhase = "connecting";
  private pending = new Map<string, (response: IpcResponse) => void>();
  private rejecters = new Map<string, (error: Error) => void>();

  constructor() {
    this.api = window.acquireVsCodeApi!();
    window.addEventListener("message", (event) => this.handle(event.data));
  }

  private daemonStart: { resolve: () => void; reject: (e: Error) => void } | null = null;

  private handle(
    msg: BridgeMessage & { phase?: ConnectionPhase; ok?: boolean; message?: string },
  ) {
    if (!msg || typeof msg !== "object") return;
    if (msg.kind === "event") {
      this.events.emit(msg.data);
    } else if (msg.kind === "response") {
      const id = msg.data.request_id;
      this.pending.get(id)?.(msg.data);
      this.pending.delete(id);
      this.rejecters.delete(id);
    } else if (msg.kind === "daemon-start-result") {
      if (msg.ok) this.daemonStart?.resolve();
      else this.daemonStart?.reject(new Error(msg.message || "could not start the daemon"));
      this.daemonStart = null;
    } else if (msg.kind === "error" && msg.id) {
      this.rejecters.get(msg.id)?.(new Error(msg.message ?? "daemon error"));
      this.pending.delete(msg.id);
      this.rejecters.delete(msg.id);
    } else if ("phase" in msg && msg.phase) {
      this.currentPhase = msg.phase;
      this.phases.emit(msg.phase);
    }
  }

  phase() {
    return this.currentPhase;
  }

  start() {
    this.api.postMessage({ kind: "start" });
  }

  stop() {
    this.api.postMessage({ kind: "stop" });
  }

  retryNow() {
    this.api.postMessage({ kind: "start" });
  }

  canStartDaemon() {
    return true;
  }

  startDaemon() {
    return new Promise<void>((resolve, reject) => {
      this.daemonStart = { resolve, reject };
      this.api.postMessage({ kind: "start-daemon" });
      setTimeout(() => {
        if (this.daemonStart) {
          this.daemonStart.reject(new Error("starting the daemon timed out"));
          this.daemonStart = null;
        }
      }, 12000);
    });
  }

  send(envelope: IpcEnvelope): Promise<IpcResponse> {
    return new Promise((resolve, reject) => {
      this.pending.set(envelope.request_id, resolve);
      this.rejecters.set(envelope.request_id, reject);
      this.api.postMessage({ kind: "request", envelope });
      setTimeout(() => {
        if (this.rejecters.has(envelope.request_id)) {
          this.rejecters.get(envelope.request_id)?.(new Error("daemon request timed out"));
          this.pending.delete(envelope.request_id);
          this.rejecters.delete(envelope.request_id);
        }
      }, 15000);
    });
  }

  onEvent(listener: (event: IpcEvent) => void) {
    return this.events.add(listener);
  }

  onPhase(listener: (phase: ConnectionPhase) => void) {
    return this.phases.add(listener);
  }
}
