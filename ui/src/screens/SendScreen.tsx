import { useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { ArrowRight, FileText, KeyRound, MessageSquare, UploadCloud } from "lucide-react";
import { useFerryClient, useRoster, useSend, useSendInline } from "@/lib/query";
import { Button, Field, Toggle } from "@/components";
import { encodeBase64, encodeBase64Bytes } from "@/lib/format";
import type { ItemKind, PickedFile } from "@/lib/ipc";

const INLINE_FILE_LIMIT_BYTES = 48 * 1024 * 1024;

function readFileBytes(file: File): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsArrayBuffer(file);
  });
}
import { ScreenHeader } from "./parts";
import { SecretComposer } from "./SecretComposer";
import { useT } from "@/lib/i18n";

type Mode = "file" | "secret" | "message";

export function SendScreen() {
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const client = useFerryClient();
  const roster = useRoster();
  const send = useSend();
  const sendInline = useSendInline();
  const t = useT();

  const mode = (params.get("kind") as Mode | null) ?? "file";
  const setMode = (m: Mode) => setParams((p) => {
    if (m === "file") p.delete("kind");
    else p.set("kind", m);
    return p;
  });

  const peers = roster.data ?? [];
  const [peerId, setPeerId] = useState(params.get("peer") ?? "");
  const selectedPeer = peers.find((p) => p.peer_id === peerId) ?? peers[0];
  const hostPath = params.get("path");
  const [picked, setPicked] = useState<PickedFile | null>(
    hostPath ? { path: hostPath, name: hostPath.split(/[\\/]/).pop() ?? hostPath } : null,
  );
  const [file, setFile] = useState<File | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const fileName = picked?.name ?? file?.name ?? null;

  async function chooseFile(input: HTMLInputElement) {
    setFileError(null);
    const fromHost = await client.pickFile();
    if (fromHost) {
      setPicked(fromHost);
      setFile(null);
      return;
    }
    input.click();
  }
  const [message, setMessage] = useState("");
  const [ttlHours, setTtlHours] = useState(24);
  const [burn, setBurn] = useState(false);
  const [notify, setNotify] = useState(false);

  const effectivePeerId = selectedPeer?.peer_id ?? "";
  const busy = send.isPending || sendInline.isPending;

  const summary = useMemo(
    () => ({
      destination: selectedPeer?.display_name ?? "—",
      state: selectedPeer
        ? selectedPeer.reachable
          ? t("common.online")
          : t("send.willQueue")
        : "—",
      payload:
        mode === "file"
          ? fileName ?? t("send.payloadFile")
          : mode === "message"
            ? t("send.payloadMessage")
            : t("send.payloadSecret"),
      ttl: t("send.ttlValue", { hours: ttlHours }),
      burn: burn ? t("common.yes") : t("common.no"),
      notify: notify ? t("common.yes") : t("common.no"),
    }),
    [selectedPeer, mode, fileName, ttlHours, burn, notify, t],
  );

  async function execute() {
    if (!effectivePeerId) return;
    const ttl_secs = Math.max(60, Math.round(ttlHours * 3600));
    if (mode === "file") {
      if (picked) {
        await send.mutateAsync({
          peer_id: effectivePeerId,
          source_path: picked.path,
          name: picked.name,
          ttl_secs,
          is_burn_after_read: burn,
          notify_on_open: notify,
        });
        navigate("/sent");
        return;
      }
      if (!file) return;
      if (file.size > INLINE_FILE_LIMIT_BYTES) {
        setFileError(t("send.tooLarge", { max: Math.floor(INLINE_FILE_LIMIT_BYTES / (1024 * 1024)) }));
        return;
      }
      let bytes: Uint8Array;
      try {
        bytes = await readFileBytes(file);
      } catch {
        setFileError(t("send.readFailed"));
        return;
      }
      await sendInline.mutateAsync({
        peer_id: effectivePeerId,
        name: file.name,
        kind: "file" as ItemKind,
        content_base64: encodeBase64Bytes(bytes),
        ttl_secs,
        is_burn_after_read: burn,
        notify_on_open: notify,
      });
      navigate("/sent");
      return;
    }
    if (mode === "message") {
      if (!message.trim()) return;
      await sendInline.mutateAsync({
        peer_id: effectivePeerId,
        name: message.slice(0, 64),
        kind: "message" as ItemKind,
        content_base64: encodeBase64(message),
        ttl_secs,
        is_burn_after_read: burn,
        notify_on_open: notify,
      });
      navigate("/sent");
    }
  }

  if (mode === "secret") {
    return (
      <div className="screen">
        <ScreenHeader title={t("send.title")} subtitle={t("send.subtitleSecret")} />
        <ModeTabs mode={mode} setMode={setMode} />
        <SecretComposer
          peers={peers}
          defaultPeerId={effectivePeerId}
          ttlHours={ttlHours}
          burn={burn}
          notify={notify}
        />
      </div>
    );
  }

  return (
    <div className="screen">
      <ScreenHeader title={t("send.title")} subtitle={t("send.subtitleFile")} />
      <ModeTabs mode={mode} setMode={setMode} />

      <div className="send-grid">
        <div className="send-form">
          <Field label={t("send.targetPeer")} hint={peers.length === 0 ? t("send.pairFirst") : undefined}>
            <select className="text-input" value={effectivePeerId} onChange={(e) => setPeerId(e.target.value)}>
              {peers.length === 0 ? <option value="">{t("send.noPairedPeers")}</option> : null}
              {[...peers]
                .sort((a, b) => Number(b.reachable) - Number(a.reachable))
                .map((p) => (
                  <option key={p.peer_id} value={p.peer_id}>
                    {p.display_name} {p.reachable ? t("send.peerOnline") : t("send.peerOffline")}
                  </option>
                ))}
            </select>
          </Field>

          {mode === "file" ? (
            <Field label={t("send.payload")}>
              <label
                className="dropzone"
                onClick={(e) => {
                  e.preventDefault();
                  const input = e.currentTarget.querySelector("input");
                  if (input) void chooseFile(input);
                }}
              >
                <UploadCloud size={20} />
                <span>{fileName ?? t("send.chooseFile")}</span>
                <input
                  type="file"
                  hidden
                  onChange={(e) => {
                    setPicked(null);
                    setFile(e.target.files?.[0] ?? null);
                  }}
                />
              </label>
              {fileError ? <p className="form-error mono">{fileError}</p> : null}
            </Field>
          ) : (
            <Field label={t("send.message")} hint={t("send.messageHint")}>
              <textarea
                className="text-input textarea"
                rows={5}
                value={message}
                onChange={(e) => setMessage(e.target.value)}
                placeholder={t("send.messagePlaceholder")}
              />
            </Field>
          )}

          <div className="param-row">
            <Field label={t("send.ttlHours")}>
              <input
                className="text-input"
                type="number"
                min={1}
                value={ttlHours}
                onChange={(e) => setTtlHours(Number(e.target.value) || 1)}
              />
            </Field>
          </div>
          <Toggle checked={burn} onChange={setBurn} label={t("send.burnLabel")} />
          <Toggle checked={notify} onChange={setNotify} label={t("send.notifyLabel")} />

          {(send.error || sendInline.error) ? (
            <p className="form-error mono">{(send.error ?? sendInline.error)?.message}</p>
          ) : null}

          <Button variant="primary" disabled={busy || !effectivePeerId} onClick={execute}>
            {busy ? t("send.sending") : t("send.execute")} <ArrowRight size={14} />
          </Button>
        </div>

        <aside className="send-summary">
          <h2>{t("send.summaryTitle")}</h2>
          <dl>
            <div>
              <dt>{t("send.destination")}</dt>
              <dd>{summary.destination}</dd>
            </div>
            <div>
              <dt>{t("send.peerState")}</dt>
              <dd>{summary.state}</dd>
            </div>
            <div>
              <dt>{t("send.payload")}</dt>
              <dd>{summary.payload}</dd>
            </div>
            <div>
              <dt>{t("send.ttl")}</dt>
              <dd>{summary.ttl}</dd>
            </div>
            <div>
              <dt>{t("send.burnOnRead")}</dt>
              <dd>{summary.burn}</dd>
            </div>
            <div>
              <dt>{t("send.notifyOnOpen")}</dt>
              <dd>{summary.notify}</dd>
            </div>
          </dl>
          <p className="send-note mono">
            {t("send.noteSealed")}
            <br />
            {t("send.noteHandshake")}
          </p>
        </aside>
      </div>
    </div>
  );
}

function ModeTabs({ mode, setMode }: { mode: Mode; setMode: (m: Mode) => void }) {
  const t = useT();
  const tabs: { id: Mode; label: string; icon: typeof FileText }[] = [
    { id: "file", label: t("send.tabFile"), icon: FileText },
    { id: "secret", label: t("send.tabSecret"), icon: KeyRound },
    { id: "message", label: t("send.tabMessage"), icon: MessageSquare },
  ];
  return (
    <div className="tabs">
      {tabs.map(({ id, label, icon: Icon }) => (
        <button key={id} className={`tab ${mode === id ? "active" : ""}`} onClick={() => setMode(id)}>
          <Icon size={14} /> {label}
        </button>
      ))}
    </div>
  );
}
