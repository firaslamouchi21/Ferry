import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from "@tanstack/react-query";
import { useFerryClient, type TransferProgress } from "./provider";
import { queryKeys } from "./keys";
import type { SendFileParams, SendInlineParams } from "@/lib/ipc";

export function useDaemonStatus() {
  const ferry = useFerryClient();
  return useQuery({ queryKey: queryKeys.status, queryFn: () => ferry.status(), refetchInterval: 10_000 });
}

export function useIdentity() {
  const ferry = useFerryClient();
  return useQuery({ queryKey: queryKeys.identity, queryFn: () => ferry.identity() });
}

export function useRoster() {
  const ferry = useFerryClient();
  return useQuery({ queryKey: queryKeys.roster, queryFn: () => ferry.rosterList() });
}

export function useInbox() {
  const ferry = useFerryClient();
  return useQuery({ queryKey: queryKeys.inbox, queryFn: () => ferry.inboxList() });
}

export function useInboxItem(itemId: string | undefined) {
  const inbox = useInbox();
  return { ...inbox, data: inbox.data?.find((item) => item.item_id === itemId) };
}

export function useSentItems() {
  const ferry = useFerryClient();
  return useQuery({ queryKey: queryKeys.sent, queryFn: () => ferry.sentList() });
}

export function useAuditLog(limit = 200) {
  const ferry = useFerryClient();
  return useQuery({ queryKey: [...queryKeys.audit, limit], queryFn: () => ferry.auditList(limit, null) });
}

export function useTransferProgress(itemId: string | undefined) {
  const client = useQueryClient();
  return itemId ? client.getQueryData<TransferProgress>(queryKeys.progress(itemId)) : undefined;
}

function useInvalidating<TArgs, TResult>(
  fn: (args: TArgs) => Promise<TResult>,
  keys: readonly (readonly unknown[])[],
): UseMutationResult<TResult, Error, TArgs> {
  const client = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      for (const key of keys) void client.invalidateQueries({ queryKey: key });
    },
  });
}

export function useSend() {
  const ferry = useFerryClient();
  return useInvalidating((p: SendFileParams) => ferry.send(p), [queryKeys.sent, queryKeys.audit]);
}

export function useSendInline() {
  const ferry = useFerryClient();
  return useInvalidating((p: SendInlineParams) => ferry.sendInline(p), [queryKeys.sent, queryKeys.audit]);
}

export function useInboxAccept() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.inboxAccept(itemId), [queryKeys.inbox, queryKeys.audit]);
}

export function useInboxReject() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.inboxReject(itemId), [queryKeys.inbox, queryKeys.audit]);
}

export function useConfirmOpened() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.confirmOpened(itemId), [queryKeys.inbox, queryKeys.audit]);
}

export function usePeerRemove() {
  const ferry = useFerryClient();
  return useInvalidating((peerId: string) => ferry.peerRemove(peerId), [queryKeys.roster]);
}

export function useSentAbort() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.sentAbort(itemId), [queryKeys.sent, queryKeys.audit]);
}

export function useSentRetry() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.sentRetry(itemId), [queryKeys.sent, queryKeys.audit]);
}

export function useRosterImport() {
  const ferry = useFerryClient();
  return useInvalidating((json: string) => ferry.rosterImport(json), [queryKeys.roster]);
}
