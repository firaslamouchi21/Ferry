export type { IpcRequest } from "@bindings/IpcRequest";
export type { IpcResponse } from "@bindings/IpcResponse";
export type { IpcEnvelope } from "@bindings/IpcEnvelope";
export type { IpcOutcome } from "@bindings/IpcOutcome";
export type { IpcResult } from "@bindings/IpcResult";
export type { IpcEvent } from "@bindings/IpcEvent";
export type { IpcResource } from "@bindings/IpcResource";
export type { FerryError } from "@bindings/FerryError";
export type { ErrorCode } from "@bindings/ErrorCode";
export type { RequestId } from "@bindings/RequestId";
export type { DaemonStatus } from "@bindings/DaemonStatus";
export type { IdentityView } from "@bindings/IdentityView";
export type { RosterPeerView } from "@bindings/RosterPeerView";
export type { RosterImportSummaryView } from "@bindings/RosterImportSummaryView";
export type { InboxItemView } from "@bindings/InboxItemView";
export type { SentItemView } from "@bindings/SentItemView";
export type { AuditEventView } from "@bindings/AuditEventView";
export type { MessageView } from "@bindings/MessageView";
export type { MessageThreadView } from "@bindings/MessageThreadView";
export type { ProviderStatusView } from "@bindings/ProviderStatusView";
export type { ProviderAuthView } from "@bindings/ProviderAuthView";
export type { GistPublishedView } from "@bindings/GistPublishedView";
export type { RosterFetchPreviewView } from "@bindings/RosterFetchPreviewView";
export type { RosterPreviewEntryView } from "@bindings/RosterPreviewEntryView";
export type { RemoteJobView } from "@bindings/RemoteJobView";
export type { RemoteJobStatusView } from "@bindings/RemoteJobStatusView";
export type { SealedImportView } from "@bindings/SealedImportView";
export type { PairMode } from "@bindings/PairMode";
export type { PairPhase } from "@bindings/PairPhase";
export type { PairBeginView } from "@bindings/PairBeginView";
export type { PairStatusView } from "@bindings/PairStatusView";
export type { TransferState } from "@bindings/TransferState";
export type { ItemKind } from "@bindings/ItemKind";
export type { PeerState } from "@bindings/PeerState";
export type { MessageState } from "@bindings/MessageState";

export const IPC_PROTOCOL_VERSION = 1;

export function num(value: bigint | number | null | undefined): number {
  if (value == null) return 0;
  return typeof value === "bigint" ? Number(value) : value;
}

export function optNum(value: bigint | number | null | undefined): number | null {
  if (value == null) return null;
  return typeof value === "bigint" ? Number(value) : value;
}
