import { useEffect, useState } from "react";
import { useFerry } from "@/lib/query";
import { useT } from "@/lib/i18n";

interface Toast {
  id: number;
  text: string;
}

let nextId = 1;

export function ToastHost() {
  const { lastEvent } = useFerry();
  const t = useT();
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(() => {
    if (!lastEvent || lastEvent.event !== "changed") return;
    if (lastEvent.params.resource !== "audit") return;
    const id = nextId++;
    const text = t("toast.activityUpdated");
    setToasts((prev) => [...prev, { id, text }]);
    const timer = setTimeout(() => setToasts((prev) => prev.filter((x) => x.id !== id)), 5000);
    return () => clearTimeout(timer);
  }, [lastEvent, t]);

  return (
    <div className="toast-host" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className="toast">
          {toast.text}
        </div>
      ))}
    </div>
  );
}
