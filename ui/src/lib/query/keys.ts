import type { IpcResource } from "@/lib/ipc";

export const queryKeys = {
  status: ["status"] as const,
  identity: ["identity"] as const,
  roster: ["roster"] as const,
  rosterExport: ["roster", "export"] as const,
  inbox: ["inbox"] as const,
  sent: ["sent"] as const,
  audit: ["audit"] as const,
  itemContent: (itemId: string) => ["item", itemId, "content"] as const,
  progress: (itemId: string) => ["progress", itemId] as const,
};

export const resourceInvalidations: Record<IpcResource, readonly (readonly string[])[]> = {
  transfer: [queryKeys.inbox, queryKeys.sent, queryKeys.audit],
  message: [queryKeys.inbox, queryKeys.audit],
  peer: [queryKeys.roster],
  roster: [queryKeys.roster, queryKeys.rosterExport],
  audit: [queryKeys.audit],
};
