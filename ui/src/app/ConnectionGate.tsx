import type { ReactNode } from "react";
import { useDaemonStatus, useFerry } from "@/lib/query";
import { IPC_PROTOCOL_VERSION } from "@/lib/ipc";
import { useT } from "@/lib/i18n";

export function ConnectionGate({ children }: { children: ReactNode }) {
  const { phase, client } = useFerry();
  const status = useDaemonStatus();
  const t = useT();

  if (phase === "connecting" && !status.data) {
    return <GateScreen title={t("gate.connectingTitle")} body={t("gate.connectingBody")} />;
  }

  if (phase === "disconnected") {
    return (
      <GateScreen
        title={t("gate.unreachableTitle")}
        body={
          client.host === "browser"
            ? t("gate.unreachableBodyBrowser")
            : t("gate.unreachableBody")
        }
      />
    );
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
