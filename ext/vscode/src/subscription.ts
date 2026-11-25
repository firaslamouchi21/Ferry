import net from "node:net";
import { envelopeFor, makeFrameReader, writeFrame } from "./framing";

const RECONNECT_DELAY_MS = 1500;

export interface SubscriptionHandlers {
  onConnected(): void;
  onDisconnected(): void;
  onEvent(event: unknown): void;
}

export class Subscription {
  private socket: net.Socket | null = null;
  private timer: NodeJS.Timeout | null = null;
  private running = false;

  constructor(
    private socketPath: () => string,
    private handlers: SubscriptionHandlers,
  ) {}

  start(): void {
    this.running = true;
    this.connect();
  }

  stop(): void {
    this.running = false;
    this.clearTimer();
    this.dropSocket();
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  private dropSocket(): void {
    const socket = this.socket;
    this.socket = null;
    socket?.destroy();
  }

  private connect(): void {
    this.clearTimer();
    this.dropSocket();
    const socket = net.connect(this.socketPath());
    this.socket = socket;
    const read = makeFrameReader((frame) => this.handlers.onEvent(JSON.parse(frame.toString())));

    socket.on("connect", () => {
      if (this.socket !== socket) return;
      writeFrame(socket, envelopeFor({ method: "subscribe" }, "sub"));
      this.handlers.onConnected();
    });
    socket.on("data", (chunk) => {
      if (this.socket !== socket) return;
      try {
        read(chunk);
      } catch {
        socket.destroy();
      }
    });
    socket.on("error", () => socket.destroy());
    socket.on("close", () => {
      if (this.socket !== socket) return;
      this.socket = null;
      this.handlers.onDisconnected();
      if (this.running) this.timer = setTimeout(() => this.connect(), RECONNECT_DELAY_MS);
    });
  }
}
