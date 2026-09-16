import { useEffect } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { Bell, TerminalSquare, UserRound } from "lucide-react";
import { SideNav } from "./SideNav";
import { ConnectionGate } from "./ConnectionGate";
import { CommandPalette } from "./CommandPalette";
import { ToastHost } from "./ToastHost";
import { useT } from "@/lib/i18n";
import { useFerry } from "@/lib/query";

function useHostNavigation() {
  const navigate = useNavigate();
  const { client } = useFerry();
  useEffect(() => {
    if (client.host !== "vscode") return;
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { kind?: unknown; route?: unknown } | null;
      if (data && data.kind === "navigate" && typeof data.route === "string" && data.route.startsWith("/")) {
        navigate(data.route);
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [client.host, navigate]);
}

export function AppShell() {
  const navigate = useNavigate();
  const t = useT();
  useHostNavigation();
  return (
    <div className="shell">
      <SideNav />
      <div className="shell-main">
        <header className="topbar">
          <button className="topbar-search" onClick={() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }))}>
            <TerminalSquare size={14} />
            <span>{t("app.search")}</span>
            <kbd>⌘K</kbd>
          </button>
          <div className="topbar-actions">
            <button className="icon-btn" aria-label={t("app.activity")} onClick={() => navigate("/activity")}>
              <Bell size={16} />
            </button>
            <button className="icon-btn" aria-label={t("app.thisDevice")} onClick={() => navigate("/settings")}>
              <UserRound size={16} />
            </button>
          </div>
        </header>
        <main className="content">
          <ConnectionGate>
            <Outlet />
          </ConnectionGate>
        </main>
      </div>
      <CommandPalette />
      <ToastHost />
    </div>
  );
}
