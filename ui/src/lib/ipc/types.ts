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
export type { SealedImportView } from "@bindings/SealedImportView";
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
