import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button, Field, Modal } from "@/components";
import { useT } from "@/lib/i18n";
import { useIdentity } from "@/lib/query/hooks";
import { useFerryClient } from "@/lib/query/provider";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "@/lib/query/keys";
import type { PairStatusView } from "@/lib/ipc/types";

type Local =
  | { stage: "choose" }
  | { stage: "listen-config" }
  | { stage: "connect-config" }
  | { stage: "running"; pairingId: string; listenAddr: string | null; code: string | null };

export function PairScreen() {
  const navigate = useNavigate();
  const t = useT();
  const ferry = useFerryClient();
  const queryClient = useQueryClient();
  const identity = useIdentity();
  const close = () => navigate("/peers");

  const [local, setLocal] = useState<Local>({ stage: "choose" });
  const [displayName, setDisplayName] = useState("");
  const [addr, setAddr] = useState("");
  const [code, setCode] = useState("");
  const [status, setStatus] = useState<PairStatusView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const persisted = useRef(false);

  useEffect(() => {
    if (!displayName && identity.data?.display_name) setDisplayName(identity.data.display_name);
  }, [identity.data, displayName]);

  useEffect(() => {
    if (local.stage !== "running") return;
    let alive = true;
    const tick = async () => {
      try {
        const s = await ferry.pairStatus(local.pairingId);
        if (!alive) return;
        setStatus(s);
        if (s.phase === "done" && !persisted.current) {
          persisted.current = true;
          queryClient.invalidateQueries({ queryKey: queryKeys.roster });
        }
      } catch (e) {
        if (alive) setError(String((e as Error).message ?? e));
      }
    };
    void tick();
    const h = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(h);
    };
  }, [local, queryClient]);

  const begin = async (mode: Parameters<typeof ferry.pairBegin>[0]) => {
    setBusy(true);
    setError(null);
    try {
      const view = await ferry.pairBegin(mode);
      setLocal({ stage: "running", pairingId: view.pairing_id, listenAddr: view.listen_addr, code: view.code });
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };

  const confirm = async (accept: boolean) => {
    if (local.stage !== "running") return;
    setBusy(true);
    try {
      await ferry.pairConfirm(local.pairingId, accept);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };

  const cancelAndReset = async () => {
    if (local.stage === "running") {
      try {
        await ferry.pairCancel(local.pairingId);
      } catch {
        void 0;
      }
    }
    persisted.current = false;
    setStatus(null);
    setError(null);
    setLocal({ stage: "choose" });
  };

  return (
    <Modal title={t("pair.title")} onClose={close}>
      <div className="pair-body">
        <p className="muted">{t("pair.intro")}</p>

        {error ? <p className="pair-error">{error}</p> : null}

        {local.stage === "choose" ? (
          <div className="pair-roles">
            <button className="pair-role" onClick={() => setLocal({ stage: "listen-config" })}>
              <span className="pair-guide-label">{t("pair.roleListen")}</span>
              <p>{t("pair.roleListenHint")}</p>
            </button>
            <button className="pair-role" onClick={() => setLocal({ stage: "connect-config" })}>
              <span className="pair-guide-label">{t("pair.roleConnect")}</span>
              <p>{t("pair.roleConnectHint")}</p>
            </button>
          </div>
        ) : null}

        {local.stage === "listen-config" ? (
          <div className="pair-form">
            <Field label={t("pair.displayName")}>
              <input className="text-input" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
            </Field>
            <div className="pair-actions">
              <Button onClick={cancelAndReset}>{t("pair.cancel")}</Button>
              <Button
                variant="primary"
                disabled={busy || !displayName}
                onClick={() => begin({ role: "listen", params: { display_name: displayName } })}
              >
                {t("pair.start")}
              </Button>
            </div>
          </div>
        ) : null}

        {local.stage === "connect-config" ? (
          <div className="pair-form">
            <Field label={t("pair.displayName")}>
              <input className="text-input" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
            </Field>
            <Field label={t("pair.addrLabel")}>
              <input
                className="text-input"
                placeholder={t("pair.addrPlaceholder")}
                value={addr}
                onChange={(e) => setAddr(e.target.value)}
              />
            </Field>
            <Field label={t("pair.codeLabel")}>
              <input
                className="text-input mono"
                placeholder={t("pair.codePlaceholder")}
                value={code}
                onChange={(e) => setCode(e.target.value)}
              />
            </Field>
            <div className="pair-actions">
              <Button onClick={cancelAndReset}>{t("pair.cancel")}</Button>
              <Button
                variant="primary"
                disabled={busy || !displayName || !addr || !code}
                onClick={() => begin({ role: "connect", params: { addr, code, display_name: displayName } })}
              >
                {busy ? t("pair.connecting") : t("pair.connect")}
              </Button>
            </div>
          </div>
        ) : null}

        {local.stage === "running" ? (
          <div className="pair-running">
            {local.listenAddr && local.code && (!status || status.phase === "awaiting_peer") ? (
              <div className="pair-waiting">
                <span className="pair-guide-label">{t("pair.waitingTitle")}</span>
                <dl className="pair-kv">
                  <dt>{t("pair.addrLabel")}</dt>
                  <dd className="mono">{local.listenAddr}</dd>
                  <dt>{t("pair.codeLabel")}</dt>
                  <dd className="mono pair-code">{local.code}</dd>
                </dl>
                <p className="muted">{t("pair.waitingBody")}</p>
              </div>
            ) : null}

            {status?.phase === "awaiting_confirmation" && status.phrase ? (
              <div className="pair-confirm">
                <span className="pair-guide-label">{t("pair.phraseTitle")}</span>
                <p className="pair-phrase mono">{status.phrase}</p>
                <p className="muted">{t("pair.phraseBody")}</p>
                <div className="pair-actions">
                  <Button disabled={busy} onClick={() => confirm(false)}>
                    {t("pair.phraseNoMatch")}
                  </Button>
                  <Button variant="primary" disabled={busy} onClick={() => confirm(true)}>
                    {t("pair.phraseMatch")}
                  </Button>
                </div>
              </div>
            ) : null}

            {status?.phase === "done" ? (
              <div className="pair-done">
                <span className="pair-guide-label">{t("pair.doneTitle")}</span>
                <p>{t("pair.doneBody", { name: status.peer_display_name ?? status.peer_fingerprint ?? "" })}</p>
                <div className="pair-actions">
                  <Button variant="primary" onClick={close}>
                    {t("common.done")}
                  </Button>
                </div>
              </div>
            ) : null}

            {status?.phase === "failed" ? (
              <div className="pair-failed">
                <span className="pair-guide-label">{t("pair.failedTitle")}</span>
                <p className="pair-error">{status.error}</p>
                <div className="pair-actions">
                  <Button onClick={close}>{t("pair.cancel")}</Button>
                  <Button variant="primary" onClick={cancelAndReset}>
                    {t("pair.retry")}
                  </Button>
                </div>
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </Modal>
  );
}
