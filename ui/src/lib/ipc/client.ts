import { MockTransport } from "./mock";
import { VsCodeTransport, WebSocketTransport, type ConnectionPhase, type Transport } from "./transport";
import {
  IPC_PROTOCOL_VERSION,
  type AuditEventView,
  type DaemonStatus,
  type ErrorCode,
  type IdentityView,
  type InboxItemView,
  type IpcEnvelope,
  type IpcEvent,
  type IpcRequest,
  type IpcResult,
  type ItemKind,
  type RemoteJobStatusView,
  type RemoteJobView,
  type MessageThreadView,
  type MessageView,
  type ProviderAuthView,
  type ProviderStatusView,
  type RosterFetchPreviewView,
  type PairBeginView,
  type PairMode,
  type PairStatusView,
  type RosterImportSummaryView,
  type RosterPeerView,
  type SealedImportView,
  type SentItemView,
} from "./types";

export class FerryIpcError extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "FerryIpcError";
  }
}

export type HostKind = "browser" | "vscode" | "mock";

export function detectHost(): HostKind {
  if (typeof window !== "undefined" && window.acquireVsCodeApi) return "vscode";
  if (typeof window !== "undefined" && new URLSearchParams(window.location.search).has("mock")) return "mock";
  return "browser";
}

function makeTransport(host: HostKind): Transport {
  switch (host) {
    case "vscode":
      return new VsCodeTransport();
    case "mock":
      return new MockTransport();
    default: {
      const proto = window.location.protocol === "https:" ? "wss" : "ws";
      return new WebSocketTransport(`${proto}://${window.location.host}/ferry-ipc`);
    }
  }
}

let counter = 0;
function nextRequestId(): string {
  counter += 1;
  return `ui-${Date.now().toString(36)}-${counter}`;
}

export interface SendFileParams {
  peer_id: string;
  source_path: string;
  name: string;
  ttl_secs: number;
  is_burn_after_read: boolean;
  notify_on_open: boolean;
}

export interface SendInlineParams {
  peer_id: string;
  name: string;
  kind: ItemKind;
  content_base64: string;
  ttl_secs: number;
  is_burn_after_read: boolean;
  notify_on_open: boolean;
}

export class FerryClient {
  readonly host: HostKind;
  private transport: Transport;

  constructor(host: HostKind = detectHost(), transport?: Transport) {
    this.host = host;
    this.transport = transport ?? makeTransport(host);
  }

  start() {
    this.transport.start();
  }
  stop() {
    this.transport.stop();
  }
  retryNow() {
    this.transport.retryNow();
  }
  canStartDaemon(): boolean {
    return this.transport.canStartDaemon();
  }
  startDaemon(): Promise<void> {
    return this.transport.startDaemon();
  }
  phase(): ConnectionPhase {
    return this.transport.phase();
  }
  onPhase(listener: (phase: ConnectionPhase) => void) {
    return this.transport.onPhase(listener);
  }
  onEvent(listener: (event: IpcEvent) => void) {
    return this.transport.onEvent(listener);
  }

  private async call(request: IpcRequest): Promise<IpcResult> {
    const envelope: IpcEnvelope = {
      ipc_protocol_version: IPC_PROTOCOL_VERSION,
      request_id: nextRequestId(),
      request,
    };
    const response = await this.transport.send(envelope);
    if (response.outcome.outcome === "err") {
      throw new FerryIpcError(response.outcome.error.code, response.outcome.error.message);
    }
    return response.outcome.value;
  }

  async status(): Promise<DaemonStatus> {
    const r = await this.call({ method: "status" });
    return (r as Extract<IpcResult, { result: "status" }>).value;
  }

  async identity(): Promise<IdentityView> {
    const r = await this.call({ method: "identity" });
    return (r as Extract<IpcResult, { result: "identity" }>).value;
  }

  async rosterList(): Promise<RosterPeerView[]> {
    const r = await this.call({ method: "roster_list" });
    return (r as Extract<IpcResult, { result: "roster_list" }>).value;
  }

  async rosterExport(): Promise<string> {
    const r = await this.call({ method: "roster_export" });
    return (r as Extract<IpcResult, { result: "roster_export" }>).value.signed_roster_json;
  }

  async rosterImport(signedRosterJson: string): Promise<RosterImportSummaryView> {
    const r = await this.call({ method: "roster_import", params: { signed_roster_json: signedRosterJson } });
    return (r as Extract<IpcResult, { result: "roster_import" }>).value;
  }

  async peerRemove(peerId: string): Promise<void> {
    await this.call({ method: "peer_remove", params: { peer_id: peerId } });
  }

  async inboxList(): Promise<InboxItemView[]> {
    const r = await this.call({ method: "inbox_list" });
    return (r as Extract<IpcResult, { result: "inbox_list" }>).value;
  }

  async inboxAccept(itemId: string): Promise<void> {
    await this.call({ method: "inbox_accept", params: { item_id: itemId } });
  }

  async inboxReject(itemId: string): Promise<void> {
    await this.call({ method: "inbox_reject", params: { item_id: itemId } });
  }

  async open(itemId: string): Promise<string> {
    const r = await this.call({ method: "open", params: { item_id: itemId } });
    return (r as Extract<IpcResult, { result: "open" }>).value.content_base64;
  }

  async confirmOpened(itemId: string): Promise<void> {
    await this.call({ method: "confirm_opened", params: { item_id: itemId } });
  }

  async send(params: SendFileParams): Promise<string> {
    const r = await this.call({ method: "send", params });
    return (r as Extract<IpcResult, { result: "send" }>).value.item_id;
  }

  async sendInline(params: SendInlineParams): Promise<string> {
    const r = await this.call({ method: "send_inline", params });
    return (r as Extract<IpcResult, { result: "send" }>).value.item_id;
  }

  async sentList(): Promise<SentItemView[]> {
    const r = await this.call({ method: "sent_list" });
    return (r as Extract<IpcResult, { result: "sent_list" }>).value;
  }

  async sentAbort(itemId: string): Promise<void> {
    await this.call({ method: "sent_abort", params: { item_id: itemId } });
  }

  async sentRetry(itemId: string): Promise<void> {
    await this.call({ method: "sent_retry", params: { item_id: itemId } });
  }

  async messageThreads(): Promise<MessageThreadView[]> {
    const r = await this.call({ method: "message_threads" });
    return (r as Extract<IpcResult, { result: "message_threads" }>).value;
  }

  async messageThread(peerId: string): Promise<MessageView[]> {
    const r = await this.call({ method: "message_thread", params: { peer_id: peerId } });
    return (r as Extract<IpcResult, { result: "message_thread" }>).value;
  }

  async providerStatus(): Promise<ProviderStatusView> {
    const r = await this.call({ method: "provider_status" });
    return (r as Extract<IpcResult, { result: "provider_status" }>).value;
  }

  async providerConnect(pat: string | null): Promise<ProviderStatusView | ProviderAuthView> {
    const r = await this.call({ method: "provider_connect", params: { pat } });
    if ((r as IpcResult).result === "provider_auth") {
      return (r as Extract<IpcResult, { result: "provider_auth" }>).value;
    }
    return (r as Extract<IpcResult, { result: "provider_status" }>).value;
  }

  async providerConnectPoll(): Promise<ProviderStatusView | null> {
    const r = await this.call({ method: "provider_connect_poll" });
    if ((r as IpcResult).result === "provider_auth_pending") return null;
    return (r as Extract<IpcResult, { result: "provider_status" }>).value;
  }

  async providerDisconnect(): Promise<void> {
    await this.call({ method: "provider_disconnect" });
  }

  async gistPublish(itemId: string): Promise<RemoteJobView> {
    const r = await this.call({ method: "gist_publish", params: { item_id: itemId } });
    return (r as Extract<IpcResult, { result: "remote_job" }>).value;
  }

  async rosterFetch(locator: string): Promise<RosterFetchPreviewView> {
    const r = await this.call({ method: "roster_fetch", params: { locator } });
    return (r as Extract<IpcResult, { result: "roster_fetch_preview" }>).value;
  }

  async rosterApplyRemote(locator: string): Promise<RemoteJobView> {
    const r = await this.call({ method: "roster_apply_remote", params: { locator } });
    return (r as Extract<IpcResult, { result: "remote_job" }>).value;
  }

  async remoteJobStatus(jobId: string): Promise<RemoteJobStatusView> {
    const r = await this.call({ method: "remote_job_status", params: { job_id: jobId } });
    return (r as Extract<IpcResult, { result: "remote_job_status" }>).value;
  }

  async auditList(limit: number, beforeMillis: number | null): Promise<AuditEventView[]> {
    const r = await this.call({
      method: "audit_list",
      params: { limit, before_millis: beforeMillis as unknown as bigint | null },
    });
    return (r as Extract<IpcResult, { result: "audit_list" }>).value;
  }

  async exportSealed(itemId: string): Promise<string> {
    const r = await this.call({ method: "export_sealed", params: { item_id: itemId } });
    return (r as Extract<IpcResult, { result: "export_sealed" }>).value.blob_base64;
  }

  async importSealed(blobBase64: string): Promise<SealedImportView> {
    const r = await this.call({ method: "import_sealed", params: { blob_base64: blobBase64 } });
    return (r as Extract<IpcResult, { result: "import_sealed" }>).value;
  }

  async pairComplete(params: Extract<IpcRequest, { method: "pair_complete" }>["params"]): Promise<void> {
    await this.call({ method: "pair_complete", params });
  }

  async pairBegin(mode: PairMode): Promise<PairBeginView> {
    const r = await this.call({ method: "pair_begin", params: { mode } });
    return (r as Extract<IpcResult, { result: "pair_begin" }>).value;
  }

  async pairStatus(pairingId: string): Promise<PairStatusView> {
    const r = await this.call({ method: "pair_status", params: { pairing_id: pairingId } });
    return (r as Extract<IpcResult, { result: "pair_status" }>).value;
  }

  async pairConfirm(pairingId: string, accept: boolean): Promise<void> {
    await this.call({ method: "pair_confirm", params: { pairing_id: pairingId, accept } });
  }

  async pairCancel(pairingId: string): Promise<void> {
    await this.call({ method: "pair_cancel", params: { pairing_id: pairingId } });
  }

  async quit(): Promise<void> {
    await this.call({ method: "quit" });
  }
}

export const ferry = new FerryClient();
