import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebSocketTransport } from "./transport";
import type { ConnectionPhase } from "./transport";
import type { IpcEnvelope } from "./types";

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  static readonly OPEN = 1;
  static readonly CLOSED = 3;

  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((raw: { data: unknown }) => void) | null = null;

  constructor(public url: string) {
    FakeWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.();
  }

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }

  deliver(msg: unknown) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }

  deliverRaw(data: unknown) {
    this.onmessage?.({ data });
  }
}

const envelope = (id: string): IpcEnvelope =>
  ({ ipc_protocol_version: 1, request_id: id, request: { method: "status" } }) as IpcEnvelope;

const okResponse = (id: string) => ({
  kind: "response",
  data: {
    request_id: id,
    outcome: {
      outcome: "ok",
      value: { result: "status", value: { protocol_version: 1, ipc_protocol_version: 1, discovery_ok: true, transport_ok: true, store_ok: true } },
    },
  },
});

let originalWebSocket: unknown;

beforeEach(() => {
  vi.useFakeTimers();
  FakeWebSocket.instances = [];
  originalWebSocket = (globalThis as Record<string, unknown>).WebSocket;
  (globalThis as Record<string, unknown>).WebSocket = FakeWebSocket;
});

afterEach(() => {
  (globalThis as Record<string, unknown>).WebSocket = originalWebSocket;
  vi.useRealTimers();
});

function connected() {
  const transport = new WebSocketTransport("ws://localhost:1/bridge");
  transport.start();
  const socket = FakeWebSocket.instances.at(-1)!;
  socket.open();
  return { transport, socket };
}

describe("WebSocketTransport", () => {
  it("subscribes for events as soon as the connection opens", () => {
    const { socket } = connected();
    const first = JSON.parse(socket.sent[0]);
    expect(first.envelope.request.method).toBe("subscribe");
  });

  it("correlates responses to their own request even when they arrive out of order", async () => {
    const { transport, socket } = connected();
    const a = transport.send(envelope("req-a"));
    const b = transport.send(envelope("req-b"));

    socket.deliver(okResponse("req-b"));
    socket.deliver(okResponse("req-a"));

    await expect(b).resolves.toMatchObject({ request_id: "req-b" });
    await expect(a).resolves.toMatchObject({ request_id: "req-a" });
  });

  it("rejects a send when the socket is not open", async () => {
    const transport = new WebSocketTransport("ws://localhost:1/bridge");
    transport.start();
    await expect(transport.send(envelope("req-1"))).rejects.toThrow(/not connected/);
  });

  it("fails every in-flight request when the connection drops", async () => {
    const { transport, socket } = connected();
    const pending = transport.send(envelope("req-1"));
    socket.close();
    await expect(pending).rejects.toThrow(/closed/);
  });

  it("times out a request the daemon never answers", async () => {
    const { transport, socket } = connected();
    const pending = transport.send(envelope("req-1"));
    expect(socket.sent.length).toBe(2);
    vi.advanceTimersByTime(15_000);
    await expect(pending).rejects.toThrow(/timed out/);
  });

  it("reports connecting → connected → disconnected as distinct phases", () => {
    const transport = new WebSocketTransport("ws://localhost:1/bridge");
    const seen: ConnectionPhase[] = [];
    transport.onPhase((p) => seen.push(p));
    transport.start();
    FakeWebSocket.instances.at(-1)!.open();
    FakeWebSocket.instances.at(-1)!.close();
    expect(seen).toEqual(["connecting", "connected", "disconnected"]);
  });

  it("reconnects with a doubling backoff that resets once a connection opens", () => {
    const { socket } = connected();
    expect(FakeWebSocket.instances).toHaveLength(1);

    socket.close();
    vi.advanceTimersByTime(499);
    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(2);

    FakeWebSocket.instances.at(-1)!.close();
    vi.advanceTimersByTime(999);
    expect(FakeWebSocket.instances, "the second wait must be longer than the first").toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(3);

    FakeWebSocket.instances.at(-1)!.open();
    FakeWebSocket.instances.at(-1)!.close();
    vi.advanceTimersByTime(500);
    expect(FakeWebSocket.instances, "a successful open resets the backoff").toHaveLength(4);
  });

  it("stops reconnecting after stop()", () => {
    const { transport, socket } = connected();
    transport.stop();
    socket.close();
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it("ignores a malformed frame instead of tearing the connection down", async () => {
    const { transport, socket } = connected();
    socket.deliverRaw("not json at all");
    socket.deliverRaw({ some: "object" });

    const pending = transport.send(envelope("req-1"));
    socket.deliver(okResponse("req-1"));
    await expect(pending).resolves.toMatchObject({ request_id: "req-1" });
  });

  it("delivers unsolicited events to event listeners", () => {
    const { transport, socket } = connected();
    const seen: unknown[] = [];
    transport.onEvent((e) => seen.push(e));
    socket.deliver({ kind: "event", data: { event: "changed", params: { resource: "roster", id: null } } });
    expect(seen).toHaveLength(1);
  });
});
