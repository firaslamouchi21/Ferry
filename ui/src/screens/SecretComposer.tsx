import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowRight } from "lucide-react";
import { useSendInline } from "@/lib/query";
import { Button, Field, MaskedValue } from "@/components";
import { encodeBase64 } from "@/lib/format";
import { useT } from "@/lib/i18n";
import type { ItemKind, RosterPeerView } from "@/lib/ipc";

type Source = "env" | "json" | "yaml";

interface Entry {
  key: string;
  value: string;
  selected: boolean;
}

function parseEnv(text: string): Entry[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#") && line.includes("="))
    .map((line) => {
      const eq = line.indexOf("=");
      return { key: line.slice(0, eq).trim(), value: line.slice(eq + 1).trim(), selected: true };
    });
}

function parseJsonish(text: string): Entry[] {
  try {
    const obj = JSON.parse(text);
    return Object.entries(obj).map(([key, value]) => ({ key, value: String(value), selected: true }));
  } catch {
    return [];
  }
}

function parseYaml(text: string): Entry[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#") && line.includes(":"))
    .map((line) => {
      const c = line.indexOf(":");
      return { key: line.slice(0, c).trim(), value: line.slice(c + 1).trim().replace(/^["']|["']$/g, ""), selected: true };
    });
}

export function SecretComposer({
  peers,
  defaultPeerId,
  ttlHours,
  burn,
  notify,
}: {
  peers: RosterPeerView[];
  defaultPeerId: string;
  ttlHours: number;
  burn: boolean;
  notify: boolean;
}) {
  const navigate = useNavigate();
  const sendInline = useSendInline();
  const t = useT();
  const [source, setSource] = useState<Source>("env");
  const [raw, setRaw] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [name, setName] = useState("secrets.env");
  const [peerId, setPeerId] = useState(defaultPeerId);

  const effectivePeerId = peers.find((p) => p.peer_id === peerId)?.peer_id ?? peers[0]?.peer_id ?? "";
  const selectedCount = entries.filter((e) => e.selected).length;

  function parse() {
    const parsed = source === "env" ? parseEnv(raw) : source === "json" ? parseJsonish(raw) : parseYaml(raw);
    setEntries(parsed);
  }

  const payload = useMemo(
    () => entries.filter((e) => e.selected).map((e) => `${e.key}=${e.value}`).join("\n"),
    [entries],
  );

  async function stage() {
    if (!effectivePeerId || selectedCount === 0) return;
    const id = await sendInline.mutateAsync({
      peer_id: effectivePeerId,
      name,
      kind: "secret" as ItemKind,
      content_base64: encodeBase64(payload),
      ttl_secs: Math.max(60, Math.round(ttlHours * 3600)),
      is_burn_after_read: burn,
      notify_on_open: notify,
    });
    navigate(`/inbox/${id}`);
  }

  return (
    <div className="composer">
      <div className="composer-import">
        <div className="tabs tabs-sm">
          {(["env", "json", "yaml"] as Source[]).map((s) => (
            <button key={s} className={`tab ${source === s ? "active" : ""}`} onClick={() => setSource(s)}>
              {s === "env" ? ".env" : s.toUpperCase()}
            </button>
          ))}
        </div>
        <textarea
          className="text-input textarea composer-textarea mono"
          rows={8}
          value={raw}
          onChange={(e) => setRaw(e.target.value)}
          placeholder={
            source === "env"
              ? "DB_URL=postgres://user:pass@localhost:5432/db\nAPI_KEY=sk_test_..."
              : source === "json"
                ? '{ "DB_URL": "...", "API_KEY": "..." }'
                : "DB_URL: ...\nAPI_KEY: ..."
          }
        />
        <Button onClick={parse}>{t("composer.parse")}</Button>
      </div>

      {entries.length > 0 ? (
        <div className="composer-keys">
          <div className="composer-keys-head">
            <span>{t("composer.keysDetected", { count: entries.length })}</span>
            <div className="row-actions">
              <Button onClick={() => setEntries((e) => e.map((x) => ({ ...x, selected: true })))}>
                {t("composer.selectAll")}
              </Button>
              <Button onClick={() => setEntries((e) => e.map((x) => ({ ...x, selected: false })))}>
                {t("composer.clear")}
              </Button>
            </div>
          </div>
          <table className="data-table">
            <thead>
              <tr>
                <th scope="col" style={{ width: 40 }}>{t("composer.colSend")}</th>
                <th scope="col">{t("composer.colKey")}</th>
                <th scope="col">{t("composer.colValue")}</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e, i) => (
                <tr key={e.key}>
                  <td>
                    <input
                      type="checkbox"
                      checked={e.selected}
                      aria-label={t("composer.include", { key: e.key })}
                      onChange={() =>
                        setEntries((all) => all.map((x, j) => (i === j ? { ...x, selected: !x.selected } : x)))
                      }
                    />
                  </td>
                  <td className="mono">{e.key}</td>
                  <td>
                    <MaskedValue value={e.value} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}

      <div className="composer-foot">
        <Field label={t("composer.payloadName")}>
          <input className="text-input" value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label={t("composer.targetPeer")}>
          <select className="text-input" value={effectivePeerId} onChange={(e) => setPeerId(e.target.value)}>
            {peers.map((p) => (
              <option key={p.peer_id} value={p.peer_id}>
                {p.display_name} {p.reachable ? t("send.peerOnline") : t("send.peerOffline")}
              </option>
            ))}
          </select>
        </Field>
        <div className="composer-submit">
          <span className="muted">{t("composer.keysSelected", { count: selectedCount })}</span>
          {sendInline.error ? <span className="form-error mono">{sendInline.error.message}</span> : null}
          <Button variant="primary" disabled={selectedCount === 0 || !effectivePeerId || sendInline.isPending} onClick={stage}>
            {t("composer.encryptAndStage")} <ArrowRight size={14} />
          </Button>
        </div>
      </div>
    </div>
  );
}
