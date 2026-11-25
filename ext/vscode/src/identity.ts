import { callDaemon } from "./framing";

interface Identity {
  fingerprint: string;
}

export function identity(socketPath: string): Promise<Identity | undefined> {
  return callDaemon<Identity>(socketPath, { method: "identity" }).catch(() => undefined);
}
