import net from "node:net";

const MAX_FRAME_BYTES = 64 * 1024 * 1024;

export function writeFrame(socket: net.Socket, payload: Buffer): void {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payload.length, 0);
  socket.write(Buffer.concat([header, payload]));
}

export function makeFrameReader(onFrame: (frame: Buffer) => void): (chunk: Buffer) => void {
  let buffer = Buffer.alloc(0);
  return (chunk: Buffer) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 4) {
      const len = buffer.readUInt32BE(0);
      if (len > MAX_FRAME_BYTES) throw new Error(`oversize frame declared: ${len}`);
      if (buffer.length < 4 + len) return;
      const frame = buffer.subarray(4, 4 + len);
      buffer = buffer.subarray(4 + len);
      onFrame(frame);
    }
  };
}
