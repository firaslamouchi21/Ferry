import { useEffect, useState } from "react";
import { useFerry } from "@/lib/query";
import { useT } from "@/lib/i18n";
import { pushToast, subscribeToasts, type Toast } from "@/lib/toast";

export function ToastHost() {
  const { lastEvent } = useFerry();
  const t = useT();
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(() => subscribeToasts(setToasts), []);

  useEffect(() => {
    if (!lastEvent || lastEvent.event !== "changed") return;
    const resource = lastEvent.params.resource;
    const key =
      resource === "roster"
        ? "toast.rosterUpdated"
        : resource === "peer"
          ? "toast.peerUpdated"
          : resource === "transfer" || resource === "message"
            ? "toast.transferUpdated"
            : resource === "audit"
              ? "toast.activityUpdated"
              : null;
    if (key) pushToast(t(key));
  }, [lastEvent, t]);

  return (
    <div className="toast-host" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast ${toast.tone === "error" ? "toast-error" : ""}`.trim()}>
          {toast.text}
        </div>
      ))}
    </div>
  );
}
