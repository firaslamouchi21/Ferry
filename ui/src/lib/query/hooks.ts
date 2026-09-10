import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from "@tanstack/react-query";
import { useFerryClient, type TransferProgress } from "./provider";
import { queryKeys } from "./keys";
import { pushToast } from "@/lib/toast";
import { encodeBase64 } from "@/lib/format";
import type { ItemKind, SendFileParams, SendInlineParams } from "@/lib/ipc";

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

export function useMessageThreads() {
  const ferry = useFerryClient();
  return useQuery({
    queryKey: queryKeys.messages,
    queryFn: () => ferry.messageThreads(),
    refetchInterval: 4_000,
  });
}

export function useMessageThread(peerId: string | undefined) {
  const ferry = useFerryClient();
  return useQuery({
    queryKey: peerId ? queryKeys.messageThread(peerId) : queryKeys.messages,
    queryFn: () => ferry.messageThread(peerId as string),
    enabled: !!peerId,
    refetchInterval: 4_000,
  });
}

export function useSendMessage() {
  const ferry = useFerryClient();
  return useInvalidating(
    (p: { peer_id: string; body: string }) =>
      ferry.sendInline({
        peer_id: p.peer_id,
        name: p.body.slice(0, 64),
        kind: "message" as ItemKind,
        content_base64: encodeBase64(p.body),
        ttl_secs: 604800,
        is_burn_after_read: false,
        notify_on_open: false,
      }),
    [queryKeys.messages, queryKeys.sent, queryKeys.audit],
  );
}

export function useProviderStatus() {
  const ferry = useFerryClient();
  return useQuery({
    queryKey: queryKeys.provider,
    queryFn: () => ferry.providerStatus(),
    refetchInterval: 8_000,
  });
}

export function useProviderConnect() {
  const ferry = useFerryClient();
  return useInvalidating((pat: string | null) => ferry.providerConnect(pat), [queryKeys.provider]);
}

export function useProviderConnectPoll() {
  const ferry = useFerryClient();
  return useInvalidating(() => ferry.providerConnectPoll(), [queryKeys.provider]);
}

export function useProviderDisconnect() {
  const ferry = useFerryClient();
  return useInvalidating(() => ferry.providerDisconnect(), [queryKeys.provider]);
}

export function useGistPublish() {
  const ferry = useFerryClient();
  return useInvalidating((itemId: string) => ferry.gistPublish(itemId), [queryKeys.audit]);
}

export function useRemoteJob(jobId: string | undefined) {
  const ferry = useFerryClient();
  return useQuery({
    queryKey: ["remote-job", jobId],
    queryFn: () => ferry.remoteJobStatus(jobId as string),
    enabled: !!jobId,
    refetchInterval: (query) => {
      const phase = query.state.data?.phase;
      return phase === "done" || phase === "failed" ? false : 1000;
    },
  });
}

export function useRosterFetch() {
  const ferry = useFerryClient();
  return useInvalidating((locator: string) => ferry.rosterFetch(locator), []);
}

export function useRosterApplyRemote() {
  const ferry = useFerryClient();
  return useInvalidating((locator: string) => ferry.rosterApplyRemote(locator), [
    queryKeys.roster,
    queryKeys.audit,
  ]);
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
    onError: (error) => {
      pushToast(error instanceof Error ? error.message : String(error), "error");
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
