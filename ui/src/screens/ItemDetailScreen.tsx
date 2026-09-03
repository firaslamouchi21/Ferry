import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { AlertTriangle, ArrowLeft } from "lucide-react";
import {
  useAuditLog,
  useConfirmOpened,
  useFerryClient,
  useInboxItem,
  useTransferProgress,
} from "@/lib/query";
import { Button, ConfirmDialog, MaskedValue, ProgressBar, StateBadge } from "@/components";
import { num, optNum, type TransferState } from "@/lib/ipc";
import { decodeBase64, formatClock, useFormat } from "@/lib/format";
import { useT, useTParts } from "@/lib/i18n";

const LIFECYCLE: TransferState[] = ["offered", "accepted", "delivered", "opened"];

export function ItemDetailScreen() {
  const { itemId } = useParams();
  const navigate = useNavigate();
  const ferry = useFerryClient();
  const item = useInboxItem(itemId);
  const audit = useAuditLog(300);
  const progress = useTransferProgress(itemId);
  const confirmOpened = useConfirmOpened();
  const t = useT();
  const tParts = useTParts();
  const fmt = useFormat();

  const [content, setContent] = useState<string | null>(null);
  const [confirmBurn, setConfirmBurn] = useState(false);

  if (item.isLoading) return <div className="screen">{t("common.loading")}</div>;
  if (!item.data) {
    return (
      <div className="screen">
        <Button onClick={() => navigate("/inbox")}>
          <ArrowLeft size={13} /> {t("item.backToInbox")}
        </Button>
        <p className="muted">{t("item.noSuchItem")}</p>
      </div>
    );
  }

  const it = item.data;
  const itemAudit = (audit.data ?? []).filter((e) => e.item_id === it.item_id);
  const lifecycleAt = (state: TransferState) =>
    itemAudit.find((e) => e.kind.includes(state))?.occurred_at_millis ?? null;

  async function reveal(force = false) {
    if (it.is_burn_after_read && !force) {
      setConfirmBurn(true);
      return;
    }
    const b64 = await ferry.open(it.item_id);
    setContent(decodeBase64(b64));
    await confirmOpened.mutateAsync(it.item_id);
  }

  const secretLines =
    content && it.kind === "secret"
      ? content.split("\n").filter(Boolean).map((line) => {
          const eq = line.indexOf("=");
          return eq > 0 ? { key: line.slice(0, eq), value: line.slice(eq + 1) } : { key: line, value: "" };
        })
      : [];

  return (
    <div className="screen">
      <button className="crumb" onClick={() => navigate("/inbox")}>
        <ArrowLeft size={13} /> {t("item.crumb", { id: it.item_id })}
      </button>

      <div className="screen-header">
        <div>
          <h1>{it.name}</h1>
          <p className="screen-subtitle">
            <span className="cap">{it.kind}</span> · {t("item.fromPeer", { peer: it.origin_display_name })}
          </p>
        </div>
        <StateBadge state={it.state} />
      </div>

      {it.is_burn_after_read ? (
        <div className="notice notice-warn">
          <AlertTriangle size={14} /> {t("item.burnNotice")}
        </div>
      ) : null}

      {it.state === "transferring" && progress ? (
        <div className="detail-progress">
          <ProgressBar value={progress.bytes} total={progress.total} />
          <span className="muted mono">
            {fmt.bytes(progress.bytes)} / {fmt.bytes(progress.total)}
          </span>
        </div>
      ) : null}

      <div className="detail-cards">
        <div className="detail-card">
          <span className="detail-label">{t("item.statusTtl")}</span>
          <span className="detail-value">
            <StateBadge state={it.state} />
          </span>
          <span className="muted">
            {it.expires_at_millis == null
              ? t("item.noExpiry")
              : t("item.expiresIn", { when: fmt.relative(num(it.expires_at_millis)) })}
          </span>
        </div>
        <div className="detail-card">
          <span className="detail-label">{t("item.integrity")}</span>
          <span className="detail-value mono ellipsis">{it.hash_hex ?? t("item.pendingVerification")}</span>
        </div>
        <div className="detail-card">
          <span className="detail-label">{t("item.payloadSize")}</span>
          <span className="detail-value">{fmt.bytes(num(it.size_bytes))}</span>
        </div>
      </div>

      <section className="detail-section">
        <div className="detail-section-head">
          <h2>{t("item.payloadContent")}</h2>
          {content == null && (it.state === "delivered" || it.state === "opened") ? (
            <Button variant="primary" onClick={() => reveal()}>
              {it.kind === "secret" ? t("item.decrypt") : t("common.open")}
            </Button>
          ) : null}
        </div>
        {content == null ? (
          <p className="muted">
            {it.state === "delivered" || it.state === "opened"
              ? t("item.heldSealed")
              : t("item.notAvailableWhile", { state: it.state })}
          </p>
        ) : it.kind === "secret" ? (
          <table className="data-table">
            <thead>
              <tr>
                <th scope="col">{t("item.colKey")}</th>
                <th scope="col">{t("item.colValue")}</th>
              </tr>
            </thead>
            <tbody>
              {secretLines.map((l) => (
                <tr key={l.key}>
                  <td className="mono">{l.key}</td>
                  <td>
                    <MaskedValue value={l.value} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <pre className="payload-pre mono">{content}</pre>
        )}
      </section>

      <section className="detail-section">
        <h2>{t("item.lifecycle")}</h2>
        <ol className="lifecycle">
          {LIFECYCLE.map((state) => {
            const at = lifecycleAt(state);
            const reached = LIFECYCLE.indexOf(it.state as TransferState) >= LIFECYCLE.indexOf(state) || at != null;
            return (
              <li key={state} className={reached ? "reached" : ""}>
                <span className="cap">{state}</span>
                <span className="muted mono">
                  {at ? formatClock(optNum(at)) : reached ? t("item.lifecycleDone") : t("item.lifecyclePending")}
                </span>
              </li>
            );
          })}
        </ol>
      </section>

      <section className="detail-section">
        <h2>{t("item.auditLog")}</h2>
        {itemAudit.length === 0 ? (
          <p className="muted">{t("item.noAuditYet")}</p>
        ) : (
          <ul className="audit-stream mono">
            {itemAudit.map((e) => (
              <li key={e.id}>
                <span className="muted">{formatClock(optNum(e.occurred_at_millis))}</span> [{e.actor}] {e.kind} — {e.outcome}
              </li>
            ))}
          </ul>
        )}
      </section>

      {confirmBurn ? (
        <ConfirmDialog
          title={t("item.openAndBurnTitle")}
          danger
          typeToConfirm="burn"
          confirmLabel={t("item.openAndBurn")}
          body={
            <p>
              {(() => {
                const parts = tParts("item.openAndBurnBody", "name");
                return (
                  <>
                    {parts[0]}
                    <strong>{it.name}</strong>
                    {parts[1]}
                  </>
                );
              })()}
            </p>
          }
          onCancel={() => setConfirmBurn(false)}
          onConfirm={() => {
            setConfirmBurn(false);
            void reveal(true);
          }}
        />
      ) : null}
    </div>
  );
}
