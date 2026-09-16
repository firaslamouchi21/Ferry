import net from "node:net";
import { makeFrameReader, writeFrame } from "./framing";

interface Identity {
  fingerprint: string;
}

export function identity(socketPath: string): Promise<Identity | undefined> {
  return new Promise((resolve) => {
    const socket = net.connect(socketPath);
    const envelope = { ipc_protocol_version: 1, request_id: `identity-${Date.now()}`, request: { method: "identity" } };
    socket.on("error", () => resolve(undefined));
    socket.on("connect", () => writeFrame(socket, Buffer.from(JSON.stringify(envelope))));
    const read = makeFrameReader((frame) => {
      socket.end();
      try {
        const response = JSON.parse(frame.toString());
        resolve(response.outcome?.outcome === "ok" ? (response.outcome.value.value as Identity) : undefined);
      } catch {
        resolve(undefined);
      }
    });
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch {
        socket.destroy();
        resolve(undefined);
      }
    });
  });
}
