import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useT } from "@/lib/i18n";

interface Command {
  id: string;
  label: string;
  run: () => void;
}

const IS_MAC =
  typeof navigator !== "undefined" && /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent);

export function CommandPalette() {
  const navigate = useNavigate();
  const t = useT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const restoreRef = useRef<HTMLElement | null>(null);

  const commands = useMemo<Command[]>(
    () => [
      { id: "peers", label: t("palette.goToPeers"), run: () => navigate("/peers") },
      { id: "send", label: t("palette.sendFile"), run: () => navigate("/send") },
      { id: "secret", label: t("palette.sendSecret"), run: () => navigate("/send?kind=secret") },
      { id: "inbox", label: t("palette.goToInbox"), run: () => navigate("/inbox") },
      { id: "sent", label: t("palette.goToSent"), run: () => navigate("/sent") },
      { id: "activity", label: t("palette.goToActivity"), run: () => navigate("/activity") },
      { id: "settings", label: t("palette.goToSettings"), run: () => navigate("/settings") },
      { id: "pair", label: t("palette.pairDevice"), run: () => navigate("/pair") },
    ],
    [navigate, t],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (open) {
      restoreRef.current = document.activeElement as HTMLElement | null;
      setQuery("");
      setSelected(0);
      inputRef.current?.focus();
    } else {
      restoreRef.current?.focus?.();
    }
  }, [open]);

  const filtered = useMemo(
    () => commands.filter((c) => c.label.toLowerCase().includes(query.toLowerCase())),
    [commands, query],
  );

  useEffect(() => {
    setSelected((s) => Math.min(s, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  if (!open) return null;

  const close = () => setOpen(false);
  const runAt = (i: number) => {
    filtered[i]?.run();
    close();
  };

  return (
    <div className="modal-scrim palette-scrim" onClick={close}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label={t("palette.placeholder")}
        onClick={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          className="palette-input"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={filtered[selected] ? `palette-opt-${filtered[selected].id}` : undefined}
          placeholder={`${t("palette.placeholder")}  ${IS_MAC ? "⌘K" : "Ctrl K"}`}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              close();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelected((s) => Math.min(s + 1, filtered.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelected((s) => Math.max(s - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              runAt(selected);
            } else if (e.key === "Tab") {
              e.preventDefault();
            }
          }}
        />
        <ul className="palette-list" id="palette-list" role="listbox">
          {filtered.map((c, i) => (
            <li key={c.id} role="presentation">
              <button
                id={`palette-opt-${c.id}`}
                role="option"
                aria-selected={i === selected}
                className={i === selected ? "active" : undefined}
                tabIndex={-1}
                onMouseMove={() => setSelected(i)}
                onClick={() => runAt(i)}
              >
                {c.label}
              </button>
            </li>
          ))}
          {filtered.length === 0 ? (
            <li className="palette-empty" role="presentation">
              {t("common.nothingHere")}
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
