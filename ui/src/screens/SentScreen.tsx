import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useAuditLog, useFerry, useSentAbort, useSentItems, useSentRetry } from "@/lib/query";
import { Button, DataTable, EmptyState, StateBadge } from "@/components";
import { num, type TransferState } from "@/lib/ipc";
import { formatClock, useFormat } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader, Tabs } from "./parts";

type Filter = "all" | "queued" | "delivered" | "dropped";

const MATCH: Record<Filter, (s: TransferState) => boolean> = {
  all: () => true,
  queued: (s) => s === "queued" || s === "offered" || s === "transferring" || s === "accepted",
  delivered: (s) => s === "delivered" || s === "opened",
  dropped: (s) => s === "failed" || s === "expired",
};

export function SentScreen() {
  const navigate = useNavigate();
  const sent = useSentItems();
  const audit = useAuditLog(100);
  const abort = useSentAbort();
  const retry = useSentRetry();
  const { lastEvent } = useFerry();
  const t = useT();
  const fmt = useFormat();
  const [filter, setFilter] = useState<Filter>("all");

  const log = useMemo(() => {
    const lines = (audit.data ?? [])
      .filter((e) => e.kind.startsWith("item.") || e.kind.startsWith("outbox."))
      .slice(0, 8)
      .map((e) => `${formatClock(num(e.occurred_at_millis))}  ${e.kind}  ${e.outcome}`);
    if (lastEvent?.event === "progress") {
      lines.unshift(
        t("sent.liveTransfer", {
          item: lastEvent.params.item_id,
          bytes: num(lastEvent.params.bytes),
          total: num(lastEvent.params.total),
        }),
      );
    }
    return lines;
  }, [audit.data, lastEvent, t]);

  return (
    <div className="screen">
      <ScreenHeader title={t("sent.title")} subtitle={t("sent.subtitle")} />

      <Async query={sent}>
        {(items) => {
          const rows = items.filter((i) => MATCH[filter](i.state));
          const queued = items.filter((i) => MATCH.queued(i.state)).length;
          const dropped = items.filter((i) => MATCH.dropped(i.state)).length;
          return (
            <>
              <div className="stat-row">
                <span>
                  {t("sent.queueDepth")} <strong>{queued}</strong>
                </span>
                <span>
                  {t("sent.dropped")} <strong>{dropped}</strong>
                </span>
              </div>
              {queued > 0 ? <p className="muted">{t("sent.queuedHint")}</p> : null}
              <Tabs
                active={filter}
                onChange={setFilter}
                tabs={[
                  { id: "all", label: t("sent.tabAll") },
                  { id: "queued", label: t("sent.tabQueued"), count: queued },
                  { id: "delivered", label: t("sent.tabDelivered") },
                  { id: "dropped", label: t("sent.tabDropped"), count: dropped },
                ]}
              />
              {rows.length === 0 ? (
                <EmptyState title={t("sent.emptyTitle")}>
                  <p>{t("sent.emptyBody")}</p>
                  <Button variant="primary" onClick={() => navigate("/send")}>
                    {t("sent.sendSomething")}
                  </Button>
                </EmptyState>
              ) : (
                <DataTable
                  rows={rows}
                  rowKey={(i) => i.item_id}
                  columns={[
                    {
                      key: "name",
                      header: t("sent.colPayload"),
                      render: (i) => (
                        <span className="cell-two-line">
                          <span className="cell-strong">{i.name}</span>
                          <span className="muted mono ellipsis">sha256:{i.hash_hex.slice(0, 12)}…</span>
                        </span>
                      ),
                    },
                    { key: "peer", header: t("sent.colDestination"), render: (i) => `@${i.peer_display_name}` },
                    { key: "size", header: t("sent.colSize"), render: (i) => fmt.bytes(num(i.size_bytes)) },
                    { key: "state", header: t("sent.colState"), render: (i) => <StateBadge state={i.state} /> },
                    {
                      key: "log",
                      header: t("sent.colLastEvent"),
                      render: (i) =>
                        i.last_error ? (
                          <span className="muted">{i.last_error}</span>
                        ) : (
                          <span className="muted">{fmt.relative(num(i.last_attempt_at_millis) || num(i.queued_at_millis))}</span>
                        ),
                    },
                    {
                      key: "actions",
                      header: "",
                      align: "end",
                      render: (i) => (
                        <span className="row-actions" onClick={(e) => e.stopPropagation()}>
                          {MATCH.queued(i.state) ? (
                            <Button variant="danger" disabled={abort.isPending} onClick={() => abort.mutate(i.item_id)}>
                              {t("sent.abort")}
                            </Button>
                          ) : null}
                          {MATCH.dropped(i.state) ? (
                            <Button disabled={retry.isPending} onClick={() => retry.mutate(i.item_id)}>
                              {t("common.retry")}
                            </Button>
                          ) : null}
                        </span>
                      ),
                    },
                  ]}
                />
              )}
            </>
          );
        }}
      </Async>

      <section className="detail-section">
        <h2>{t("sent.systemLog")}</h2>
        <ul className="audit-stream mono">
          {log.length === 0 ? (
            <li className="muted">{t("common.idle")}</li>
          ) : (
            log.map((line, i) => <li key={i}>&gt; {line}</li>)
          )}
        </ul>
      </section>
    </div>
  );
}
