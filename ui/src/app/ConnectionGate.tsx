import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, Play, RotateCw } from "lucide-react";
import { useDaemonStatus, useFerry } from "@/lib/query";
import { IPC_PROTOCOL_VERSION } from "@/lib/ipc";
import { useT } from "@/lib/i18n";

export function ConnectionGate({ children }: { children: ReactNode }) {
  const { phase } = useFerry();
  const status = useDaemonStatus();
  const t = useT();

  const [seenDown, setSeenDown] = useState(false);
  useEffect(() => {
    if (phase === "disconnected") setSeenDown(true);
    else if (phase === "connected") setSeenDown(false);
  }, [phase]);

  if (phase !== "connected" && seenDown) {
    return <DaemonDownPanel connecting={phase === "connecting"} />;
  }

  if (phase === "connecting" && !status.data) {
    return <GateScreen title={t("gate.connectingTitle")} body={t("gate.connectingBody")} />;
  }

  const daemonVersion = status.data?.ipc_protocol_version;
  if (daemonVersion != null && daemonVersion !== IPC_PROTOCOL_VERSION) {
    return (
      <GateScreen
        title={t("gate.versionMismatchTitle")}
        body={t("gate.versionMismatchBody", { app: IPC_PROTOCOL_VERSION, daemon: daemonVersion })}
      />
    );
  }

  return <>{children}</>;
}

const START_COMMAND = "ferry daemon start";

function DaemonDownPanel({ connecting }: { connecting: boolean }) {
  const { client } = useFerry();
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const autoTried = useRef(false);

  useEffect(() => {
    if (!connecting) setBusy(false);
  }, [connecting]);

  const start = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await client.startDaemon();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }, [client]);

  useEffect(() => {
    if (autoTried.current || !client.canStartDaemon()) return;
    autoTried.current = true;
    void start();
  }, [client, start]);

  const onStart = start;

  function onCopy() {
    void navigator.clipboard?.writeText(START_COMMAND).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <div className="gate">
      <div className="gate-card">
        <span className={`daemon-dot ${connecting ? "pending" : "off"}`} />
        <h1>{t("gate.downTitle")}</h1>
        <p>{t("gate.downBody")}</p>

        {client.canStartDaemon() && (
          <button className="btn btn-primary" onClick={onStart} disabled={busy}>
            <Play size={14} /> {busy ? t("gate.starting") : t("gate.startButton")}
          </button>
        )}

        {error && <p className="gate-error">{error}</p>}

        <div className="gate-manual">
          <p>{t("gate.manualHint")}</p>
          <div className="gate-cmd">
            <code>{START_COMMAND}</code>
            <button className="btn" onClick={onCopy} aria-label={t("gate.copy")}>
              <Copy size={13} /> {copied ? t("gate.copied") : t("gate.copy")}
            </button>
          </div>
        </div>

        <button className="btn" onClick={() => client.retryNow()}>
          <RotateCw size={13} /> {t("gate.retry")}
        </button>
      </div>
    </div>
  );
}

function GateScreen({ title, body }: { title: string; body: string }) {
  return (
    <div className="gate">
      <div className="gate-card">
        <span className="daemon-dot off" />
        <h1>{title}</h1>
        <p>{body}</p>
      </div>
    </div>
  );
}
