import { Fragment, useEffect, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { Button } from "./primitives";
import { useT, useTParts } from "@/lib/i18n";

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const t = useT();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-scrim" onClick={onClose}>
      <div
        className={`modal ${wide ? "modal-wide" : ""}`.trim()}
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" aria-label={t("common.close")} onClick={onClose}>
            <X size={16} />
          </button>
        </header>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  );
}

export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  danger,
  typeToConfirm,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  typeToConfirm?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const tParts = useTParts();
  const [typed, setTyped] = useState("");
  const ready = !typeToConfirm || typed.trim() === typeToConfirm;
  const confirmParts = tParts("common.typeToConfirm", "token");
  return (
    <Modal title={title} onClose={onCancel}>
      <div className="confirm-body">{body}</div>
      {typeToConfirm ? (
        <label className="field">
          <span className="field-label">
            {confirmParts.map((part, i) => (
              <Fragment key={i}>
                {part}
                {i < confirmParts.length - 1 ? <code>{typeToConfirm}</code> : null}
              </Fragment>
            ))}
          </span>
          <input className="text-input mono" value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus />
        </label>
      ) : null}
      <div className="modal-actions">
        <Button variant="ghost" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button variant={danger ? "danger" : "primary"} disabled={!ready} onClick={onConfirm}>
          {confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}
