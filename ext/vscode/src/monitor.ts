import * as vscode from "vscode";
import { callDaemon } from "./framing";
import { configuredSocketPath } from "./daemon-bridge";
import { Subscription } from "./subscription";

interface InboxItem {
  item_id: string;
  peer_id: string;
  origin_display_name: string;
  kind: "file" | "secret" | "message";
  name: string;
  state: string;
}

const REFRESH_DELAY_MS = 300;

function isWaiting(item: InboxItem): boolean {
  return item.state === "delivered" || item.state === "offered";
}

function describe(item: InboxItem): string {
  const from = item.origin_display_name;
  if (item.state === "offered") return `${from} wants to send you ${item.kind === "file" ? `“${item.name}”` : `a ${item.kind}`}`;
  if (item.kind === "file") return `received “${item.name}” from ${from}`;
  return `received a ${item.kind} from ${from}`;
}

function routeFor(item: InboxItem): string {
  return item.kind === "message" ? `/messages/${encodeURIComponent(item.peer_id)}` : `/inbox/${encodeURIComponent(item.item_id)}`;
}

export class DaemonMonitor implements vscode.Disposable {
  private subscription: Subscription;
  private seen: Set<string> | undefined;
  private refreshTimer: NodeJS.Timeout | undefined;
  private connected = false;

  constructor(
    private status: vscode.StatusBarItem,
    private open: (route: string) => void,
    private onConnect: () => void,
  ) {
    this.subscription = new Subscription(configuredSocketPath, {
      onConnected: () => {
        this.connected = true;
        this.seen = undefined;
        this.scheduleRefresh();
        this.onConnect();
      },
      onDisconnected: () => {
        this.connected = false;
        this.render(0);
      },
      onEvent: (event) => {
        if ((event as { event?: string })?.event === "changed") this.scheduleRefresh();
      },
    });
    this.render(0);
  }

  start(): void {
    this.subscription.start();
  }

  private render(waiting: number): void {
    this.status.text = this.connected ? `$(radio-tower) Ferry${waiting > 0 ? ` ${waiting}` : ""}` : "$(debug-disconnect) Ferry";
    this.status.tooltip = !this.connected
      ? "Ferry: the daemon is not running — click to open Ferry and start it"
      : waiting > 0
        ? `Ferry: ${waiting} item${waiting === 1 ? "" : "s"} waiting in your inbox`
        : "Ferry: the daemon is running";
  }

  private scheduleRefresh(): void {
    if (this.refreshTimer) return;
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = undefined;
      void this.refresh();
    }, REFRESH_DELAY_MS);
  }

  private async refresh(): Promise<void> {
    let items: InboxItem[];
    try {
      items = await callDaemon<InboxItem[]>(configuredSocketPath(), { method: "inbox_list" });
    } catch {
      return;
    }
    const waiting = items.filter(isWaiting);
    this.render(waiting.filter((item) => item.kind !== "message").length);

    const key = (item: InboxItem) => `${item.item_id}:${item.state}`;
    const known = this.seen;
    this.seen = new Set(waiting.map(key));
    if (!known) return;
    if (!vscode.workspace.getConfiguration("ferry").get<boolean>("notifyOnReceive", true)) return;
    for (const item of waiting) {
      if (!known.has(key(item))) void this.notify(item);
    }
  }

  private async notify(item: InboxItem): Promise<void> {
    const choice = await vscode.window.showInformationMessage(`Ferry: ${describe(item)}`, "Open");
    if (choice === "Open") this.open(routeFor(item));
  }

  dispose(): void {
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.subscription.stop();
  }
}
