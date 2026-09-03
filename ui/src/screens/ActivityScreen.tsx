import { useMemo, useState } from "react";
import { Download } from "lucide-react";
import { useAuditLog } from "@/lib/query";
import { Button, DataTable, EmptyState, StatusChip } from "@/components";
import { num } from "@/lib/ipc";
import { formatTime } from "@/lib/format";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader } from "./parts";

export function ActivityScreen() {
  const [limit, setLimit] = useState(200);
  const audit = useAuditLog(limit);
  const t = useT();
  const [filter, setFilter] = useState("");

  const rows = useMemo(() => {
    const all = audit.data ?? [];
    const q = filter.toLowerCase();
    return q ? all.filter((e) => `${e.actor} ${e.kind} ${e.item_id ?? ""} ${e.outcome}`.toLowerCase().includes(q)) : all;
  }, [audit.data, filter]);

  function exportCsv() {
    const header = "time,actor,kind,item_id,outcome";
    const lines = rows.map(
      (e) => `${new Date(num(e.occurred_at_millis)).toISOString()},${e.actor},${e.kind},${e.item_id ?? ""},${e.outcome}`,
    );
    const blob = new Blob([[header, ...lines].join("\n")], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ferry-activity.csv";
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="screen">
      <ScreenHeader
        title={t("activity.title")}
        subtitle={t("activity.subtitle")}
        actions={
          <Button onClick={exportCsv}>
            <Download size={13} /> {t("activity.exportCsv")}
          </Button>
        }
      />

      <input
        className="text-input filter-input"
        placeholder={t("activity.filterPlaceholder")}
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />

      <Async query={audit}>
        {(events) =>
          events.length === 0 ? (
            <EmptyState title={t("activity.emptyTitle")}>
              <p>{t("activity.emptyBody")}</p>
            </EmptyState>
          ) : (
            <>
              <DataTable
                rows={rows}
                rowKey={(e) => e.id}
                columns={[
                  { key: "time", header: t("activity.colTime"), render: (e) => <span className="mono">{formatTime(num(e.occurred_at_millis))}</span> },
                  { key: "actor", header: t("activity.colActor"), render: (e) => e.actor },
                  { key: "kind", header: t("activity.colEvent"), render: (e) => <span className="mono">{e.kind}</span> },
                  { key: "item", header: t("activity.colItem"), render: (e) => <span className="mono">{e.item_id ?? "—"}</span> },
                  {
                    key: "outcome",
                    header: t("activity.colOutcome"),
                    render: (e) => (
                      <StatusChip tone={e.outcome === "ok" ? "online" : e.outcome === "denied" || e.outcome === "dropped" ? "error" : "warn"}>
                        {e.outcome}
                      </StatusChip>
                    ),
                  },
                ]}
                footer={
                  <button className="link-btn" onClick={() => setLimit((l) => l + 200)}>
                    {t("activity.loadOlder")}
                  </button>
                }
              />
            </>
          )
        }
      </Async>
    </div>
  );
}
