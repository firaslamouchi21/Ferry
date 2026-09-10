import { useEffect, useRef, useState } from "react";
import { useProviderConnect, useProviderConnectPoll } from "@/lib/query";
import { Button, CopyButton, Field } from "@/components";
import { useT } from "@/lib/i18n";
import type { ProviderAuthView } from "@/lib/ipc";

export function ProviderConnect() {
  const t = useT();
  const connect = useProviderConnect();
  const poll = useProviderConnectPoll();
  const [pat, setPat] = useState("");
  const [device, setDevice] = useState<ProviderAuthView | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    const d = connect.data;
    if (d && "user_code" in d) setDevice(d);
  }, [connect.data]);

  useEffect(() => {
    if (!device) return;
    const interval = Math.max(1, device.interval_secs) * 1000;
    timer.current = setInterval(() => poll.mutate(), interval);
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [device, poll]);

  useEffect(() => {
    if (poll.data && "connected" in poll.data && poll.data.connected && timer.current) {
      clearInterval(timer.current);
    }
  }, [poll.data]);

  if (device) {
    return (
      <div className="panel stack">
        <p>{t("provider.deviceOpen")}</p>
        <p>
          <a href={device.verification_uri} target="_blank" rel="noreferrer">
            {device.verification_uri}
          </a>
        </p>
        <div className="row">
          <code className="device-code">{device.user_code}</code>
          <CopyButton text={device.user_code} />
        </div>
        <p className="muted">{t("provider.deviceWaiting")}</p>
      </div>
    );
  }

  return (
    <div className="panel stack">
      <p>{t("provider.connectIntro")}</p>
      <Button disabled={connect.isPending} onClick={() => connect.mutate(null)}>
        {t("provider.connectDevice")}
      </Button>
      <details>
        <summary>{t("provider.patSummary")}</summary>
        <Field label={t("provider.patLabel")} hint={t("provider.patHint")}>
          <input
            type="password"
            className="text-input mono"
            value={pat}
            onChange={(e) => setPat(e.target.value)}
            autoComplete="off"
          />
        </Field>
        <Button disabled={!pat || connect.isPending} onClick={() => connect.mutate(pat)}>
          {t("provider.connectPat")}
        </Button>
      </details>
    </div>
  );
}
