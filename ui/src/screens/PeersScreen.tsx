import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Download, MonitorSmartphone, MoreVertical, Send, Upload, Link2 } from "lucide-react";
import { useFerryClient, usePeerRemove, useRoster, useRosterImport } from "@/lib/query";
import { Button, ConfirmDialog, DataTable, EmptyState, Fingerprint, RelativeTime, StatusChip } from "@/components";
import { num } from "@/lib/ipc";
import type { RosterPeerView } from "@/lib/ipc";
import { Async, ScreenHeader } from "./parts";
import { useT, useTParts } from "@/lib/i18n";

export function PeersScreen() {
  const navigate = useNavigate();
  const ferry = useFerryClient();
  const roster = useRoster();
  const removePeer = usePeerRemove();
  const rosterImport = useRosterImport();
  const t = useT();
  const tParts = useTParts();
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [removing, setRemoving] = useState<RosterPeerView | null>(null);

  async function exportRoster() {
    const json = await ferry.rosterExport();
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ferry-roster.json";
    a.click();
    URL.revokeObjectURL(url);
  }

  function importRoster() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "application/json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (file) rosterImport.mutate(await file.text());
    };
    input.click();
  }

  return (
    <div className="screen">
      <ScreenHeader
        title={t("peers.title")}
        subtitle={t("peers.subtitle")}
        actions={
          <>
            <Button onClick={importRoster}>
              <Download size={13} /> {t("peers.import")}
            </Button>
            <Button onClick={exportRoster}>
              <Upload size={13} /> {t("peers.export")}
            </Button>
            <Button variant="primary" onClick={() => navigate("/pair")}>
              <Link2 size={13} /> {t("peers.pairDevice")}
            </Button>
          </>
        }
      />

      {rosterImport.data ? (
        <div className="notice">
          {(() => {
            const parts = tParts("peers.importedNotice", "signer", {
              added: rosterImport.data.added,
              total: rosterImport.data.peer_count,
              skipped: rosterImport.data.skipped_existing,
            });
            return (
              <>
                {parts[0]}
                <code>{rosterImport.data.signer_verifying_key_hex}</code>
                {parts[1]}
              </>
            );
          })()}
        </div>
      ) : null}

      <Async query={roster}>
        {(peers) =>
          peers.length === 0 ? (
            <EmptyState title={t("peers.emptyTitle")}>
              <p>{t("peers.emptyBody")}</p>
              <Button variant="primary" onClick={() => navigate("/pair")}>
                <Link2 size={13} /> {t("peers.pairADevice")}
              </Button>
            </EmptyState>
          ) : (
            <DataTable
              rows={peers}
              rowKey={(p) => p.peer_id}
              columns={[
                {
                  key: "host",
                  header: t("peers.colHostname"),
                  render: (p) => (
                    <span className="cell-strong">
                      <MonitorSmartphone size={15} className={p.reachable ? "ic-online" : "ic-offline"} />
                      {p.display_name}
                    </span>
                  ),
                },
                { key: "fp", header: t("peers.colFingerprint"), render: (p) => <Fingerprint value={p.fingerprint_short} /> },
                {
                  key: "status",
                  header: t("peers.colStatus"),
                  render: (p) => {
                    const seen = p.last_seen_millis == null ? null : num(p.last_seen_millis);
                    const veryRecent = seen != null && Date.now() - seen < 45_000;
                    const tone = veryRecent ? "online" : p.reachable ? "warn" : "offline";
                    const label = veryRecent
                      ? t("common.online")
                      : p.reachable
                        ? t("peers.recentlySeen")
                        : t("common.offline");
                    return (
                      <span className="cell-status">
                        <StatusChip tone={tone}>{label}</StatusChip>
                        <span className="muted">
                          <RelativeTime millis={seen} />
                        </span>
                      </span>
                    );
                  },
                },
                {
                  key: "actions",
                  header: "",
                  align: "end",
                  render: (p) => (
                    <span className="row-actions">
                      <Button
                        title={p.reachable ? t("peers.sendToPeer") : t("peers.sendWhenOffline")}
                        onClick={() => navigate(`/send?peer=${encodeURIComponent(p.peer_id)}`)}
                      >
                        <Send size={13} /> {t("peers.send")}
                      </Button>
                      <div className="menu-anchor">
                        <button
                          className="icon-btn"
                          aria-label={t("common.more")}
                          onClick={() => setMenuFor(menuFor === p.peer_id ? null : p.peer_id)}
                        >
                          <MoreVertical size={15} />
                        </button>
                        {menuFor === p.peer_id ? (
                          <div className="menu">
                            <button
                              className="menu-item danger"
                              onClick={() => {
                                setMenuFor(null);
                                setRemoving(p);
                              }}
                            >
                              {t("peers.removePeer")}
                            </button>
                          </div>
                        ) : null}
                      </div>
                    </span>
                  ),
                },
              ]}
              footer={
                <span className="muted">
                  {t("peers.footer", {
                    count: peers.length,
                    reachable: peers.filter((p) => p.reachable).length,
                  })}
                </span>
              }
            />
          )
        }
      </Async>

      {removing ? (
        <ConfirmDialog
          title={t("peers.removeTitle")}
          danger
          typeToConfirm={removing.display_name}
          confirmLabel={t("peers.removePeer")}
          body={
            <p>
              {(() => {
                const parts = tParts("peers.removeBody", "name");
                return (
                  <>
                    {parts[0]}
                    <strong>{removing.display_name}</strong>
                    {parts[1]}
                  </>
                );
              })()}
            </p>
          }
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            removePeer.mutate(removing.peer_id);
            setRemoving(null);
          }}
        />
      ) : null}
    </div>
  );
}
