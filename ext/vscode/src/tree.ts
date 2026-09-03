import * as vscode from "vscode";
import net from "node:net";
import { makeFrameReader, writeFrame } from "./framing";

interface RosterPeer {
  peer_id: string;
  display_name: string;
  reachable: boolean;
  fingerprint_short: string;
}

interface Identity {
  fingerprint: string;
}

function call<T>(socketPath: string, method: string): Promise<T> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(socketPath);
    const envelope = { ipc_protocol_version: 1, request_id: `tree-${Date.now()}`, request: { method } };
    socket.on("error", reject);
    socket.on("connect", () => writeFrame(socket, Buffer.from(JSON.stringify(envelope))));
    const read = makeFrameReader((frame) => {
      const response = JSON.parse(frame.toString());
      socket.end();
      if (response.outcome?.outcome === "ok") resolve(response.outcome.value.value as T);
      else reject(new Error(response.outcome?.error?.message ?? "ipc error"));
    });
    socket.on("data", (chunk) => {
      try {
        read(chunk);
      } catch (err) {
        reject(err);
      }
    });
  });
}

export class PeersTreeProvider implements vscode.TreeDataProvider<RosterPeer> {
  private emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.emitter.event;

  constructor(private socketPath: string) {}

  refresh() {
    this.emitter.fire();
  }

  async identity(): Promise<Identity | undefined> {
    try {
      return await call<Identity>(this.socketPath, "identity");
    } catch {
      return undefined;
    }
  }

  getTreeItem(peer: RosterPeer): vscode.TreeItem {
    const item = new vscode.TreeItem(peer.display_name);
    item.description = `${peer.fingerprint_short} · ${peer.reachable ? "online" : "offline"}`;
    item.iconPath = new vscode.ThemeIcon(peer.reachable ? "vm-active" : "vm-outline");
    item.contextValue = "ferryPeer";
    return item;
  }

  async getChildren(): Promise<RosterPeer[]> {
    try {
      return await call<RosterPeer[]>(this.socketPath, "roster_list");
    } catch {
      return [];
    }
  }
}
