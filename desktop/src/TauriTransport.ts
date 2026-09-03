import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConnectionPhase, IpcEnvelope, IpcEvent, IpcResponse, Transport } from "@ferry/ui/lib/ipc";

export class TauriTransport implements Transport {
  private eventListeners = new Set<(event: IpcEvent) => void>();
  private phaseListeners = new Set<(phase: ConnectionPhase) => void>();
  private currentPhase: ConnectionPhase = "connecting";
  private unlisten: (() => void) | null = null;

  phase(): ConnectionPhase {
    return this.currentPhase;
  }

  async start(): Promise<void> {
    this.unlisten = await listen<IpcEvent>("ferry://event", (e) => {
      for (const l of this.eventListeners) l(e.payload);
    });
    try {
      await invoke("ipc_subscribe");
      this.setPhase("connected");
    } catch {
      this.setPhase("disconnected");
    }
  }

  stop(): void {
    this.unlisten?.();
    this.unlisten = null;
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
