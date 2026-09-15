import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConnectionPhase, IpcEnvelope, IpcEvent, IpcResponse, Transport } from "@ferry/ui/lib/ipc";

export class TauriTransport implements Transport {
  private eventListeners = new Set<(event: IpcEvent) => void>();
  private phaseListeners = new Set<(phase: ConnectionPhase) => void>();
  private currentPhase: ConnectionPhase = "connecting";
  private unlisten: (() => void) | null = null;
  private unlistenClosed: (() => void) | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private generation = 0;

  phase(): ConnectionPhase {
    return this.currentPhase;
  }

  async start(): Promise<void> {
    const gen = ++this.generation;
    try {
      const unlisten = await listen<IpcEvent>("ferry://event", (e) => {
        if (gen !== this.generation) return;
        for (const l of this.eventListeners) l(e.payload);
      });
      const unlistenClosed = await listen("ferry://subscribe-closed", () => {
        if (gen !== this.generation) return;
        this.setPhase("disconnected");
        this.retryTimer = setTimeout(() => void this.attempt(gen), 1500);
      });
      if (gen !== this.generation) {
        unlisten();
        unlistenClosed();
        return;
      }
      this.unlisten = unlisten;
      this.unlistenClosed = unlistenClosed;
      void this.attempt(gen);
    } catch {
      if (gen !== this.generation) return;
      this.setPhase("disconnected");
      this.retryTimer = setTimeout(() => void this.start(), 1500);
    }
  }

  private async attempt(gen: number): Promise<void> {
    if (gen !== this.generation) return;
    try {
      await invoke("ipc_subscribe");
      if (gen !== this.generation) return;
      this.setPhase("connected");
    } catch {
      if (gen !== this.generation) return;
      this.setPhase("disconnected");
      this.retryTimer = setTimeout(() => void this.attempt(gen), 1500);
    }
  }

  stop(): void {
    this.generation++;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.unlisten?.();
    this.unlisten = null;
    this.unlistenClosed?.();
    this.unlistenClosed = null;
  }

  retryNow(): void {
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    void this.attempt(this.generation);
  }

  canStartDaemon(): boolean {
    return true;
  }

  async startDaemon(): Promise<void> {
    await invoke("start_daemon");
    this.retryNow();
  }

  private setPhase(phase: ConnectionPhase) {
    this.currentPhase = phase;
    for (const l of this.phaseListeners) l(phase);
  }

  async send(envelope: IpcEnvelope): Promise<IpcResponse> {
    return invoke<IpcResponse>("ipc_request", { envelope });
  }

  onEvent(listener: (event: IpcEvent) => void) {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onPhase(listener: (phase: ConnectionPhase) => void) {
    this.phaseListeners.add(listener);
    return () => this.phaseListeners.delete(listener);
  }
}
