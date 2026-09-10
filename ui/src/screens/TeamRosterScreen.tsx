import { useEffect, useState } from "react";
import { useProviderStatus, useRemoteJob, useRosterApplyRemote, useRosterFetch } from "@/lib/query";
import { Button, EmptyState, Field, Fingerprint, StatusChip } from "@/components";
import { ConfirmDialog } from "@/components/Modal";
import { useT } from "@/lib/i18n";
import { Async, ScreenHeader } from "./parts";
import { ProviderConnect } from "./ProviderConnect";

export function TeamRosterScreen() {
  const t = useT();
  const status = useProviderStatus();
  const fetchRoster = useRosterFetch();
  const apply = useRosterApplyRemote();
  const [locator, setLocator] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [jobId, setJobId] = useState<string | undefined>(undefined);
  const job = useRemoteJob(jobId);

  useEffect(() => {
    if (apply.data?.job_id) setJobId(apply.data.job_id);
  }, [apply.data]);

  const preview = fetchRoster.data;

  return (
    <div className="screen">
      <ScreenHeader title={t("provider.rosterTitle")} subtitle={t("provider.rosterSubtitle")} />
      <Async query={status}>
        {(s) => {
          if (!s.enabled) {
            return (
              <EmptyState title={t("provider.offTitle")}>{t("provider.offBody")}</EmptyState>
            );
          }
          if (!s.connected) return <ProviderConnect />;
          return (
            <div className="stack">
              <Field label={t("provider.rosterLocatorLabel")} hint={t("provider.rosterLocatorHint")}>
                <input
                  className="text-input mono"
                  value={locator}
                  onChange={(e) => setLocator(e.target.value)}
                  placeholder="acme/team-config/ferry-roster.json"
                />
              </Field>
              <Button disabled={!locator || fetchRoster.isPending} onClick={() => fetchRoster.mutate(locator)}>
                {t("provider.rosterFetch")}
              </Button>

              {preview ? (
                <div className="panel stack">
                  <div className="row">
                    <span>{t("provider.rosterSigner")}</span>
                    <Fingerprint value={preview.signer_verifying_key_hex} />
                    {preview.known_signer ? (
                      <StatusChip tone="online">{t("provider.rosterKnown")}</StatusChip>
                    ) : (
                      <StatusChip tone="warn">{t("provider.rosterUnknown")}</StatusChip>
                    )}
                  </div>
                  <p className="muted">
                    {t("provider.rosterDiff", { adds: preview.adds, present: preview.already_present })}
                  </p>
                  <ul className="list">
                    {preview.entries.map((e) => (
                      <li key={e.peer_id} className="list-row">
                        <span>{e.already_present ? "=" : "+"}</span>
                        <span className="mono">{e.peer_id}</span>
                        <span>{e.display_name}</span>
                      </li>
                    ))}
                  </ul>
                  <Button disabled={preview.adds === 0} onClick={() => setConfirming(true)}>
                    {t("provider.rosterImport", { adds: preview.adds })}
                  </Button>
                  {jobId && job.data?.phase === "running" ? (
                    <p className="muted">{t("provider.importing")}</p>
                  ) : null}
                  {job.data?.phase === "failed" ? (
                    <p className="error-text">
                      {t("provider.importFailed", { error: job.data.error ?? "" })}
                    </p>
                  ) : null}
                  {job.data?.phase === "done" ? <p>{job.data.result_summary ?? ""}</p> : null}
                </div>
              ) : null}
            </div>
          );
        }}
      </Async>

      {confirming ? (
        <ConfirmDialog
          title={t("provider.rosterConfirmTitle")}
          typeToConfirm="import"
          confirmLabel={t("provider.rosterConfirmLabel")}
          body={t("provider.rosterConfirmBody")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            apply.mutate(locator);
          }}
        />
      ) : null}
    </div>
  );
}
