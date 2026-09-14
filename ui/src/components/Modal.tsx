import { Fragment, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { Button } from "./primitives";
import { useT, useTParts } from "@/lib/i18n";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

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
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    const first = dialog?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? dialog)?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCloseRef.current();
        return;
      }
      if (e.key !== "Tab" || !dialog) return;
      const items = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.offsetParent !== null,
      );
      if (items.length === 0) {
        e.preventDefault();
        dialog.focus();
        return;
      }
      const active = document.activeElement as HTMLElement;
      const idx = items.indexOf(active);
      if (e.shiftKey && (idx <= 0 || idx === -1)) {
        e.preventDefault();
        items[items.length - 1].focus();
      } else if (!e.shiftKey && (idx === items.length - 1 || idx === -1)) {
        e.preventDefault();
        items[0].focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previouslyFocused?.focus?.();
    };
  }, []);

  return (
    <div className="modal-scrim" onClick={onClose}>
      <div
        ref={dialogRef}
        className={`modal ${wide ? "modal-wide" : ""}`.trim()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="modal-head">
          <h2 id={titleId}>{title}</h2>
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
