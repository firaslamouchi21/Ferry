import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useT } from "@/lib/i18n";

interface Command {
  id: string;
  label: string;
  run: () => void;
}

export function CommandPalette() {
  const navigate = useNavigate();
  const t = useT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

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
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!open) return null;

  const filtered = commands.filter((c) => c.label.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="modal-scrim palette-scrim" onClick={() => setOpen(false)}>
      <div className="palette" onClick={(e) => e.stopPropagation()}>
        <input
          className="palette-input"
          autoFocus
          placeholder={t("palette.placeholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && filtered[0]) {
              filtered[0].run();
              setOpen(false);
            }
          }}
        />
        <ul className="palette-list">
          {filtered.map((c) => (
            <li key={c.id}>
              <button
                onClick={() => {
                  c.run();
                  setOpen(false);
                }}
              >
                {c.label}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
