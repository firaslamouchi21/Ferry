import { NavLink } from "react-router-dom";
import { Activity, Inbox, Send, Settings, Upload, Users, Plus, BookText, Bug, GitPullRequest, MessageSquare, Share2, GitBranch } from "lucide-react";
import { useDaemonStatus, useFerry } from "@/lib/query";
import { useT } from "@/lib/i18n";
import type { MessageKey } from "@/lib/i18n";

const NAV: { to: string; label: MessageKey; icon: typeof Users }[] = [
  { to: "/peers", label: "nav.peers", icon: Users },
  { to: "/send", label: "nav.send", icon: Send },
  { to: "/messages", label: "nav.messages", icon: MessageSquare },
  { to: "/inbox", label: "nav.inbox", icon: Inbox },
  { to: "/sent", label: "nav.sent", icon: Upload },
  { to: "/activity", label: "nav.activity", icon: Activity },
  { to: "/publish", label: "nav.publish", icon: Share2 },
  { to: "/team-roster", label: "nav.teamRoster", icon: GitBranch },
  { to: "/settings", label: "nav.settings", icon: Settings },
];

export function SideNav() {
  const { phase } = useFerry();
  const status = useDaemonStatus();
  const t = useT();
  const online = phase === "connected";
  const version = status.data?.protocol_version;

  const dotClass = online ? "on" : phase === "connecting" ? "pending" : "off";
  const statusText = online
    ? version
      ? t("nav.connectedVersion", { version })
      : t("nav.connected")
    : phase === "connecting"
      ? t("nav.connecting")
      : t("nav.disconnected");

  return (
    <nav className="sidenav" aria-label={t("nav.primary")}>
      <div className="sidenav-head">
        <div className="brand">
          <span className={`daemon-dot ${dotClass}`} title={statusText} />
          <span className="brand-name">{t("app.name")}</span>
        </div>
        <div className="brand-sub mono">{statusText}</div>
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
        <a className="nav-item" href="https://github.com/firaslamouchi21/Ferry#readme" target="_blank" rel="noreferrer">
          <BookText size={16} />
          <span>{t("app.docs")}</span>
        </a>
        <a
          className="nav-item"
          href="https://github.com/firaslamouchi21/Ferry/issues/new"
          target="_blank"
          rel="noreferrer"
        >
          <Bug size={16} />
          <span>{t("app.reportIssue")}</span>
        </a>
        <a
          className="nav-item"
          href="https://github.com/firaslamouchi21/Ferry/blob/main/CONTRIBUTING.md"
          target="_blank"
          rel="noreferrer"
        >
          <GitPullRequest size={16} />
          <span>{t("app.contribute")}</span>
        </a>
      </div>
    </nav>
  );
}
