import { useEffect, useState } from "react";
import { useGistPublish, useProviderStatus, useRemoteJob, useSentItems } from "@/lib/query";
import { Button, CopyButton, EmptyState } from "@/components";
import { ConfirmDialog } from "@/components/Modal";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader } from "./parts";
import { ProviderConnect } from "./ProviderConnect";

export function PublishScreen() {
  const t = useT();
  const status = useProviderStatus();
  const sent = useSentItems();
  const publish = useGistPublish();
  const [confirming, setConfirming] = useState<string | null>(null);
  const [jobId, setJobId] = useState<string | undefined>(undefined);
  const job = useRemoteJob(jobId);
  const publishedUrl = job.data?.phase === "done" ? job.data.result_url : null;

  useEffect(() => {
    if (publish.data?.job_id) setJobId(publish.data.job_id);
  }, [publish.data]);

  return (
    <div className="screen">
      <ScreenHeader title={t("provider.publishTitle")} subtitle={t("provider.publishSubtitle")} />
      <Async query={status}>
        {(s) => {
          if (!s.enabled) {
            return (
              <EmptyState title={t("provider.offTitle")}>{t("provider.offBody")}</EmptyState>
            );
          }
          if (!s.connected) return <ProviderConnect />;
          const delivered = (sent.data ?? []).filter(
            (i) => i.state === "delivered" || i.state === "opened",
          );
          return (
            <div className="stack">
              <p className="screen-subtitle">{t("provider.connectedAs", { login: s.login ?? "?" })}</p>
              {delivered.length === 0 ? (
                <EmptyState title={t("provider.publishNothing")}>
                  {t("provider.publishNothingHint")}
                </EmptyState>
              ) : (
                <ul className="list">
                  {delivered.map((i) => (
                    <li key={i.item_id} className="list-row">
                      <span>{i.name}</span>
                      <span className="mono muted">→ {i.peer_display_name}</span>
                      <Button onClick={() => setConfirming(i.item_id)}>
                        {t("provider.publishAction")}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
              {jobId && !publishedUrl && job.data?.phase !== "failed" ? (
                <p className="muted">{t("provider.publishing")}</p>
              ) : null}
              {job.data?.phase === "failed" ? (
                <p className="error-text">{t("provider.publishFailed", { error: job.data.error ?? "" })}</p>
              ) : null}
              {publishedUrl ? (
                <div className="panel">
                  <p>{t("provider.publishedLabel")}</p>
                  <div className="row">
                    <code>{publishedUrl}</code>
                    <CopyButton text={publishedUrl} sensitive />
                  </div>
                </div>
              ) : null}
            </div>
          );
        }}
      </Async>

      {confirming ? (
        <ConfirmDialog
          title={t("provider.publishConfirmTitle")}
          typeToConfirm="publish"
          confirmLabel={t("provider.publishConfirmLabel")}
          danger
          body={t("provider.publishConfirmBody")}
          onCancel={() => setConfirming(null)}
          onConfirm={() => {
            const id = confirming;
            setConfirming(null);
            if (id) publish.mutate(id);
          }}
        />
      ) : null}
    </div>
  );
}
