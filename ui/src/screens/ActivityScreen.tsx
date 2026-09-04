import { useMemo, useState } from "react";
import { Download } from "lucide-react";
import { useAuditLog } from "@/lib/query";
import { Button, DataTable, EmptyState, StatusChip } from "@/components";
import { detectHost, num } from "@/lib/ipc";
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

  const [exportNote, setExportNote] = useState<string | null>(null);

  async function exportCsv() {
    const header = "time,actor,kind,item_id,outcome";
    const lines = rows.map(
      (e) => `${new Date(num(e.occurred_at_millis)).toISOString()},${e.actor},${e.kind},${e.item_id ?? ""},${e.outcome}`,
    );
    const csv = [header, ...lines].join("\n");

    let downloaded = false;
    if (detectHost() !== "vscode") {
      try {
        const url = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = "ferry-activity.csv";
        a.click();
        URL.revokeObjectURL(url);
        downloaded = true;
      } catch {
        void 0;
      }
    }
    if (!downloaded) {
      try {
        await navigator.clipboard.writeText(csv);
        setExportNote(t("activity.exportCopied"));
      } catch {
        setExportNote(t("activity.exportFailed"));
      }
      setTimeout(() => setExportNote(null), 6000);
    }
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

      {exportNote ? <p className="muted export-note">{exportNote}</p> : null}

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
