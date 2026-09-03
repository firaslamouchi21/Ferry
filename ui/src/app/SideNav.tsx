import { NavLink } from "react-router-dom";
import { Activity, Inbox, Send, Settings, Upload, Users, Plus, BookText, LifeBuoy } from "lucide-react";
import { useDaemonStatus, useFerry } from "@/lib/query";
import { useT } from "@/lib/i18n";
import type { MessageKey } from "@/lib/i18n";

const NAV: { to: string; label: MessageKey; icon: typeof Users }[] = [
  { to: "/peers", label: "nav.peers", icon: Users },
  { to: "/send", label: "nav.send", icon: Send },
  { to: "/inbox", label: "nav.inbox", icon: Inbox },
  { to: "/sent", label: "nav.sent", icon: Upload },
  { to: "/activity", label: "nav.activity", icon: Activity },
  { to: "/settings", label: "nav.settings", icon: Settings },
];

export function SideNav() {
  const { phase } = useFerry();
  const status = useDaemonStatus();
  const t = useT();
  const online = phase === "connected";
  const version = status.data?.protocol_version;

  return (
    <nav className="sidenav" aria-label="Primary">
      <div className="sidenav-head">
        <div className="brand">
          <span className={`daemon-dot ${online ? "on" : "off"}`} title="Daemon status" />
          <span className="brand-name">{t("app.name")}</span>
        </div>
        <div className="brand-sub mono">
          {online
            ? version
              ? t("nav.connectedVersion", { version })
              : t("nav.connected")
            : phase === "connecting"
              ? t("nav.connecting")
              : t("nav.disconnected")}
        </div>
      </div>

      <div className="sidenav-cta">
        <NavLink to="/send" className="btn btn-primary sidenav-new">
          <Plus size={14} /> {t("nav.newTransfer")}
        </NavLink>
      </div>

      <div className="sidenav-links">
        {NAV.map(({ to, label, icon: Icon }) => (
          <NavLink key={to} to={to} className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}>
            <Icon size={17} />
            <span>{t(label)}</span>
          </NavLink>
        ))}
      </div>

      <div className="sidenav-foot">
        <a className="nav-item" href="https://github.com/ferry" target="_blank" rel="noreferrer">
          <BookText size={16} />
          <span>{t("app.docs")}</span>
        </a>
        <a className="nav-item" href="https://github.com/ferry/issues" target="_blank" rel="noreferrer">
          <LifeBuoy size={16} />
          <span>{t("app.support")}</span>
        </a>
      </div>
    </nav>
  );
}
