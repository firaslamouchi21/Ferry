import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Send } from "lucide-react";
import { useMessageThread, useMessageThreads, useRoster, useSendMessage } from "@/lib/query";
import { Button, EmptyState } from "@/components";
import { num } from "@/lib/ipc";
import { useFormat } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader } from "./parts";

export function MessagesScreen() {
  const { peerId } = useParams();
  const navigate = useNavigate();
  const threads = useMessageThreads();
  const roster = useRoster();
  const t = useT();

  const peers = roster.data ?? [];

  return (
    <div className="screen">
      <ScreenHeader title={t("nav.messages")} subtitle={t("messages.subtitle")} />
      <div className="messages">
        <div className="messages-list">
          <Async query={threads} skeletonRows={4}>
            {(list) => {
              const rows = [
                ...list,
                ...peers
                  .filter((p) => !list.some((th) => th.peer_id === p.peer_id))
                  .map((p) => ({
                    peer_id: p.peer_id,
                    display_name: p.display_name,
                    reachable: p.reachable,
                    last_body: "",
                    last_at_millis: 0n as unknown as bigint,
                    count: 0,
                  })),
              ];
              if (rows.length === 0) {
                return <EmptyState title={t("messages.noPeers")}>{t("messages.noPeersHint")}</EmptyState>;
              }
              return rows.map((th) => (
                <button
                  key={th.peer_id}
                  className={`messages-peer ${th.peer_id === peerId ? "active" : ""}`}
                  onClick={() => navigate(`/messages/${th.peer_id}`)}
                >
                  <div className="messages-peer-top">
                    <span className={`daemon-dot ${th.reachable ? "on" : "off"}`} />
                    <span className="messages-peer-name">{th.display_name}</span>
                  </div>
                  <span className="messages-peer-preview">
                    {th.last_body || t("messages.noneYet")}
                  </span>
                </button>
              ));
            }}
          </Async>
        </div>
        <div className="messages-pane">
          {peerId ? (
            <Conversation peerId={peerId} />
          ) : (
            <EmptyState title={t("messages.pickAPeer")} />
          )}
        </div>
      </div>
    </div>
  );
}

function Conversation({ peerId }: { peerId: string }) {
  const thread = useMessageThread(peerId);
  const send = useSendMessage();
  const t = useT();
  const fmt = useFormat();
  const [draft, setDraft] = useState("");
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [thread.data?.length]);

  function submit() {
    const body = draft.trim();
    if (!body) return;
    send.mutate({ peer_id: peerId, body });
    setDraft("");
  }

  return (
    <div className="conversation">
      <div className="conversation-log">
        <Async query={thread} skeletonRows={3}>
          {(messages) =>
            messages.length === 0 ? (
              <EmptyState title={t("messages.noneYet")} />
            ) : (
              messages.map((m) => (
                <div key={m.item_id} className={`bubble ${m.outbound ? "out" : "in"}`}>
                  <p>{m.body}</p>
                  <span className="bubble-meta">
                    {fmt.clock(num(m.at_millis))}
                    {m.outbound ? ` · ${m.state}` : ""}
                  </span>
                </div>
              ))
            )
          }
        </Async>
        <div ref={endRef} />
      </div>
      <div className="conversation-compose">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={t("messages.placeholder")}
          rows={2}
        />
        <Button onClick={submit} disabled={!draft.trim() || send.isPending}>
          <Send size={14} /> {t("messages.send")}
        </Button>
      </div>
    </div>
  );
}
