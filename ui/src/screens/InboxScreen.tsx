import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { AlertTriangle } from "lucide-react";
import { useInbox, useInboxAccept, useInboxReject } from "@/lib/query";
import { Button, DataTable, EmptyState, StateBadge } from "@/components";
import { num } from "@/lib/ipc";
import type { InboxItemView, TransferState } from "@/lib/ipc";
import { useFormat } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader, Tabs } from "./parts";

type Filter = "pending" | "delivered" | "opened" | "expired";

const MATCH: Record<Filter, TransferState[]> = {
  pending: ["offered", "accepted", "transferring", "queued"],
  delivered: ["delivered"],
  opened: ["opened"],
  expired: ["expired", "failed"],
};

export function InboxScreen() {
  const navigate = useNavigate();
  const inbox = useInbox();
  const accept = useInboxAccept();
  const reject = useInboxReject();
  const t = useT();
  const fmt = useFormat();
  const [filter, setFilter] = useState<Filter>("pending");

  return (
    <div className="screen">
      <ScreenHeader title={t("inbox.title")} subtitle={t("inbox.subtitle")} />

      <Async query={inbox}>
        {(items) => {
          const counts = {
            pending: items.filter((i) => MATCH.pending.includes(i.state)).length,
            delivered: items.filter((i) => MATCH.delivered.includes(i.state)).length,
            opened: items.filter((i) => MATCH.opened.includes(i.state)).length,
            expired: items.filter((i) => MATCH.expired.includes(i.state)).length,
          };
          const rows = items.filter((i) => MATCH[filter].includes(i.state));
          return (
            <>
              <Tabs
                active={filter}
                onChange={setFilter}
                tabs={[
                  { id: "pending", label: t("inbox.tabPending"), count: counts.pending },
                  { id: "delivered", label: t("inbox.tabDelivered"), count: counts.delivered },
                  { id: "opened", label: t("inbox.tabOpened"), count: counts.opened },
                  { id: "expired", label: t("inbox.tabExpired"), count: counts.expired },
                ]}
              />
              {rows.length === 0 ? (
                <EmptyState title={t("inbox.emptyTitle")}>
                  <p>{t("inbox.emptyBody")}</p>
                </EmptyState>
              ) : (
                <DataTable
                  rows={rows}
                  rowKey={(i) => i.item_id}
                  onRowClick={(i) => navigate(`/inbox/${i.item_id}`)}
                  columns={[
                    {
                      key: "payload",
                      header: t("inbox.colPayload"),
                      render: (i: InboxItemView) => (
                        <span className="cell-strong">
                          {i.is_burn_after_read ? (
                            <AlertTriangle size={14} className="ic-warn" aria-label={t("inbox.burnsOnOpen")} />
                          ) : null}
                          {i.name}
                        </span>
                      ),
                    },
                    { key: "source", header: t("inbox.colSource"), render: (i) => i.origin_display_name },
                    { key: "type", header: t("inbox.colType"), render: (i) => <span className="cap">{i.kind}</span> },
                    { key: "size", header: t("inbox.colSize"), render: (i) => fmt.bytes(num(i.size_bytes)) },
                    { key: "state", header: t("inbox.colState"), render: (i) => <StateBadge state={i.state} /> },
                    {
                      key: "actions",
                      header: "",
                      align: "end",
                      render: (i) => (
                        <span className="row-actions" onClick={(e) => e.stopPropagation()}>
                          {i.state === "offered" ? (
                            <>
                              <Button variant="primary" disabled={accept.isPending} onClick={() => accept.mutate(i.item_id)}>
                                {t("inbox.accept")}
                              </Button>
                              <Button variant="danger" disabled={reject.isPending} onClick={() => reject.mutate(i.item_id)}>
                                {t("inbox.reject")}
                              </Button>
                            </>
                          ) : i.state === "delivered" ? (
                            <Button onClick={() => navigate(`/inbox/${i.item_id}`)}>{t("common.open")}</Button>
                          ) : i.state === "opened" ? (
                            <Button onClick={() => navigate(`/inbox/${i.item_id}`)}>{t("common.view")}</Button>
                          ) : null}
                        </span>
                      ),
                    },
                  ]}
                  footer={<span className="muted">{t("inbox.footer", { count: items.length })}</span>}
                />
              )}
            </>
          );
        }}
      </Async>
    </div>
  );
}
