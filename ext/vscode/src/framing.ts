import net from "node:net";

export const IPC_PROTOCOL_VERSION = 1;

const MAX_FRAME_BYTES = 64 * 1024 * 1024;

export function writeFrame(socket: net.Socket, payload: Buffer): void {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payload.length, 0);
  socket.write(Buffer.concat([header, payload]));
}

export function makeFrameReader(onFrame: (frame: Buffer) => void): (chunk: Buffer) => void {
  let chunks: Buffer[] = [];
  let buffered = 0;
  return (chunk: Buffer) => {
    chunks.push(chunk);
    buffered += chunk.length;
    while (buffered >= 4) {
      if (chunks[0].length < 4) chunks = [Buffer.concat(chunks)];
      const length = chunks[0].readUInt32BE(0);
      if (length > MAX_FRAME_BYTES) throw new Error(`oversize frame declared: ${length}`);
      if (buffered < 4 + length) return;
      const joined = chunks.length === 1 ? chunks[0] : Buffer.concat(chunks);
      const frame = joined.subarray(4, 4 + length);
      const rest = joined.subarray(4 + length);
      chunks = rest.length > 0 ? [rest] : [];
      buffered = rest.length;
      onFrame(frame);
    }
  };
}

export function envelopeFor(request: unknown, prefix: string): Buffer {
  return Buffer.from(
    JSON.stringify({
      ipc_protocol_version: IPC_PROTOCOL_VERSION,
      request_id: `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`,
      request,
    }),
  );
}

export function callDaemon<T>(socketPath: string, request: unknown, timeoutMs = 5000): Promise<T> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(socketPath);
    let settled = false;
    const settle = (finish: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      finish();
    };
    const timer = setTimeout(() => settle(() => reject(new Error("the daemon did not answer in time"))), timeoutMs);
    const read = makeFrameReader((frame) => {
      settle(() => {
        try {
          const response = JSON.parse(frame.toString());
          if (response.outcome?.outcome === "ok") resolve(response.outcome.value?.value as T);
          else reject(new Error(response.outcome?.error?.message ?? "the daemon refused the request"));
        } catch (err) {
          reject(err as Error);
        }
      });
    });
    socket.on("connect", () => writeFrame(socket, envelopeFor(request, "vscode")));
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch (err) {
        settle(() => reject(err as Error));
      }
    });
    socket.on("error", (err) => settle(() => reject(err)));
    socket.on("close", () => settle(() => reject(new Error("the daemon closed the connection without answering"))));
  });
}
