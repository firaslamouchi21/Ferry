import type { ConnectionPhase, Transport } from "./transport";
import type { InboxItemView, IpcEnvelope, IpcEvent, IpcResponse, IpcResult, SentItemView } from "./types";

const b = (n: number) => n as unknown as bigint;

const PEER_IDS = [
  "a1b2c3d4e5f6071829aabbccddeeff00112233445566778899aabbccddeeff001",
  "e5f6789001a2b3c4d5e6f7089900aabbccddeeff00112233445566778899aabbc",
  "9a8b7c6d5e4f302118273645ffeeddccbbaa00998877665544332211aabbccdd",
] as const;

const seedPeers = () => [
  {
    peer_id: PEER_IDS[0],
    display_name: "dev-box-02",
    paired_at_millis: b(Date.now() - 1000 * 60 * 60 * 24 * 9),
    fingerprint_short: "a1b2 c3d4",
    reachable: true,
    last_seen_millis: b(Date.now() - 1000 * 12),
  },
  {
    peer_id: PEER_IDS[1],
    display_name: "office-linux",
    paired_at_millis: b(Date.now() - 1000 * 60 * 60 * 24 * 30),
    fingerprint_short: "e5f6 7890",
    reachable: false,
    last_seen_millis: b(Date.now() - 1000 * 60 * 60 * 2),
  },
  {
    peer_id: PEER_IDS[2],
    display_name: "ci-runner-88",
    paired_at_millis: b(Date.now() - 1000 * 60 * 60 * 24 * 4),
    fingerprint_short: "9a8b 7c6d",
    reachable: true,
    last_seen_millis: b(Date.now() - 1000 * 60 * 3),
  },
];

const seedInbox = (): InboxItemView[] => [
  {
    item_id: "IT_1001",
    peer_id: PEER_IDS[0],
    origin_display_name: "dev-box-02",
    kind: "secret" as const,
    name: "db-creds.env",
    state: "delivered" as const,
    size_bytes: b(412),
    is_burn_after_read: true,
    hash_hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    received_at_millis: b(Date.now() - 1000 * 60 * 6),
    expires_at_millis: b(Date.now() + 1000 * 60 * 60 * 23),
  },
  {
    item_id: "IT_1002",
    peer_id: PEER_IDS[1],
    origin_display_name: "office-linux",
    kind: "file" as const,
    name: "build-artifacts.tar.gz",
    state: "transferring" as const,
    size_bytes: b(48_210_944),
    is_burn_after_read: false,
    hash_hex: null,
    received_at_millis: b(Date.now() - 1000 * 30),
    expires_at_millis: null,
  },
  {
    item_id: "IT_1003",
    peer_id: PEER_IDS[0],
    origin_display_name: "dev-box-02",
    kind: "message" as const,
    name: "deploy window moved to 22:00 UTC",
    state: "opened" as const,
    size_bytes: b(31),
    is_burn_after_read: false,
    hash_hex: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    received_at_millis: b(Date.now() - 1000 * 60 * 60),
    expires_at_millis: b(Date.now() + 1000 * 60 * 60 * 20),
  },
  {
    item_id: "IT_1004",
    peer_id: PEER_IDS[2],
    origin_display_name: "ci-runner-88",
    kind: "secret" as const,
    name: "api-keys.json",
    state: "offered" as const,
    size_bytes: b(880),
    is_burn_after_read: true,
    hash_hex: null,
    received_at_millis: b(Date.now() - 1000 * 8),
    expires_at_millis: null,
  },
  {
    item_id: "IT_1005",
    peer_id: PEER_IDS[0],
    origin_display_name: "dev-box-02",
    kind: "secret" as const,
    name: "auth-token.jwt",
    state: "expired" as const,
    size_bytes: b(240),
    is_burn_after_read: true,
    hash_hex: null,
    received_at_millis: b(Date.now() - 1000 * 60 * 60 * 26),
    expires_at_millis: b(Date.now() - 1000 * 60 * 60 * 2),
  },
];

const seedSent = (): SentItemView[] => [
  {
    item_id: "OT_2001",
    peer_id: PEER_IDS[1],
    peer_display_name: "office-linux",
    name: "report.pdf",
    hash_hex: "8f43a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e",
    kind: "file" as const,
    state: "queued" as const,
    size_bytes: b(2_400_120),
    queued_at_millis: b(Date.now() - 1000 * 60 * 4),
    last_attempt_at_millis: b(Date.now() - 1000 * 40),
    last_error: "peer offline, retrying",
  },
  {
    item_id: "OT_2002",
    peer_id: PEER_IDS[0],
    peer_display_name: "dev-box-02",
    name: "notes.md",
    hash_hex: "1a9c2b3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9",
    kind: "file" as const,
    state: "delivered" as const,
    size_bytes: b(4_312),
    queued_at_millis: b(Date.now() - 1000 * 60 * 12),
    last_attempt_at_millis: b(Date.now() - 1000 * 60 * 11),
    last_error: null,
  },
  {
    item_id: "OT_2003",
    peer_id: PEER_IDS[0],
    peer_display_name: "dev-box-02",
    name: "staging.env",
    hash_hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    kind: "secret" as const,
    state: "opened" as const,
    size_bytes: b(512),
    queued_at_millis: b(Date.now() - 1000 * 60 * 18),
    last_attempt_at_millis: b(Date.now() - 1000 * 60 * 17),
    last_error: null,
  },
  {
    item_id: "OT_2004",
    peer_id: PEER_IDS[1],
    peer_display_name: "office-linux",
    name: "old-backup.zip",
    hash_hex: "deadbeef00112233445566778899aabbccddeeff00112233445566778899aabb",
    kind: "file" as const,
    state: "failed" as const,
    size_bytes: b(91_400_000),
    queued_at_millis: b(Date.now() - 1000 * 60 * 60 * 25),
    last_attempt_at_millis: b(Date.now() - 1000 * 60 * 61),
    last_error: "outbox TTL expired (24h)",
  },
];

const seedAudit = () => [
  ["you", "item.opened", "IT_1003", "ok", 45_000],
  ["dev-box-02", "item.delivered", "OT_2002", "ok", 312_000],
  ["you", "pair.completed", null, "ok", 921_000],
  ["system", "daemon.sync", null, "ok", 1_820_000],
  ["you", "item.sent", "OT_2001", "queued", 240_000],
  ["ci-runner-88", "item.offered", "IT_1004", "ok", 8_000],
  ["system", "outbox.expired", "OT_2004", "dropped", 3_660_000],
  ["ci-runner-88", "auth.request", null, "denied", 5_400_000],
  ["you", "settings.update", null, "ok", 7_200_000],
].map(([actor, kind, item_id, outcome, ago], i) => ({
  id: `AE_${1000 + i}`,
  actor: actor as string,
  kind: kind as string,
  item_id: item_id as string | null,
  occurred_at_millis: b(Date.now() - (ago as number)),
  outcome: outcome as string,
}));

const identity = {
  fingerprint: "a1b2c3d4e5f6-7890-a1b2-c3d4-e5f67890a1b2c3d4",
  signing_key_hex: "9f3c1a77b2e4d508a1b2c3d4e5f67890a1b2c3d4e5f67890a1b2c3d4e5f67890a",
  sealing_key: "age1qz8x7c6v5b4n3m2a1s0d9f8g7h6j5k4l3p2o1i0u9y8t7r6e5w4q3",
  display_name: "this-macbook",
  listen_port: 47821,
  data_dir: "~/.local/share/ferry",
  protocol_version: 1,
  auto_accept_from_roster: true,
};

function ok(value: IpcResult): (request_id: string) => IpcResponse {
  return (request_id) => ({ request_id, outcome: { outcome: "ok", value } });
}

export class MockTransport implements Transport {
  private eventListeners = new Set<(event: IpcEvent) => void>();
  private phaseListeners = new Set<(phase: ConnectionPhase) => void>();
  private started = false;
  private progressTimer: ReturnType<typeof setInterval> | null = null;
  private peers = seedPeers();
  private inbox = seedInbox();
  private sent = seedSent();
  private audit = seedAudit();
  private pairings = new Map<
    string,
    { phase: "awaiting_peer" | "awaiting_confirmation" | "done" | "failed"; ticks: number; accepted?: boolean }
  >();

  phase(): ConnectionPhase {
    return "connected";
  }

  start() {
    if (this.started) return;
    this.started = true;
    queueMicrotask(() => {
      for (const listener of this.phaseListeners) listener("connected");
    });
    this.progressTimer = setInterval(() => {
      const item = this.inbox.find((i) => i.state === "transferring");
      if (item) {
        const total = Number(item.size_bytes);
        const done = Math.min(total, Math.round(total * (0.2 + Math.random() * 0.5)));
        this.emit({ event: "progress", params: { item_id: item.item_id, bytes: b(done), total: b(total) } });
      }
    }, 2500);
  }

  stop() {
    if (this.progressTimer !== null) {
      clearInterval(this.progressTimer);
      this.progressTimer = null;
    }
    this.started = false;
  }

  emit(event: IpcEvent) {
    for (const listener of this.eventListeners) listener(event);
  }

  async send(envelope: IpcEnvelope): Promise<IpcResponse> {
    const { request } = envelope;
    const id = envelope.request_id;
    await new Promise((r) => setTimeout(r, 90 + Math.random() * 120));
    switch (request.method) {
      case "status":
        return ok({
          result: "status",
          value: { protocol_version: 1, discovery_ok: true, transport_ok: true, store_ok: true },
        })(id);
      case "identity":
        return ok({ result: "identity", value: identity })(id);
      case "roster_list":
        return ok({ result: "roster_list", value: this.peers })(id);
      case "peer_remove":
        this.peers = this.peers.filter((p) => p.peer_id !== request.params.peer_id);
        this.emit({ event: "changed", params: { resource: "roster", id: null } });
        return ok({ result: "ack" })(id);
      case "roster_export":
        return ok({
          result: "roster_export",
          value: { signed_roster_json: JSON.stringify({ signer: identity.fingerprint, peers: this.peers }, null, 2) },
        })(id);
      case "roster_import":
        return ok({
          result: "roster_import",
          value: { signer_verifying_key_hex: "9f3c1a77", peer_count: 4, added: 2, skipped_existing: 2 },
        })(id);
      case "inbox_list":
        return ok({ result: "inbox_list", value: this.inbox })(id);
      case "inbox_accept":
        this.inbox = this.inbox.map((i) => (i.item_id === request.params.item_id ? { ...i, state: "delivered" as const } : i));
        this.emit({ event: "changed", params: { resource: "transfer", id: request.params.item_id } });
        return ok({ result: "ack" })(id);
      case "inbox_reject":
        this.inbox = this.inbox.map((i) => (i.item_id === request.params.item_id ? { ...i, state: "failed" as const } : i));
        this.emit({ event: "changed", params: { resource: "transfer", id: request.params.item_id } });
        return ok({ result: "ack" })(id);
      case "open":
        return ok({
          result: "open",
          value: {
            content_base64: btoa(
              "POSTGRES_URL=postgres://ferry:s3cr3t@db.internal:5432/app\nREDIS_URL=redis://cache.internal:6379\nSIGNING_SECRET=sk_live_4eC39HqLyjWDarjtT1zdp7dc",
            ),
          },
        })(id);
      case "confirm_opened":
        this.inbox = this.inbox.map((i) => (i.item_id === request.params.item_id ? { ...i, state: "opened" as const } : i));
        this.emit({ event: "changed", params: { resource: "transfer", id: request.params.item_id } });
        return ok({ result: "ack" })(id);
      case "send":
      case "send_inline": {
        const itemId = `OT_${Math.floor(Math.random() * 9000 + 3000)}`;
        this.emit({ event: "changed", params: { resource: "transfer", id: itemId } });
        return ok({ result: "send", value: { item_id: itemId } })(id);
      }
      case "sent_list":
        return ok({ result: "sent_list", value: this.sent })(id);
      case "sent_abort":
        this.sent = this.sent.filter((s) => s.item_id !== request.params.item_id);
        this.emit({ event: "changed", params: { resource: "transfer", id: request.params.item_id } });
        return ok({ result: "ack" })(id);
      case "sent_retry":
        this.sent = this.sent.map((s) => (s.item_id === request.params.item_id ? { ...s, state: "queued" as const } : s));
        this.emit({ event: "changed", params: { resource: "transfer", id: request.params.item_id } });
        return ok({ result: "ack" })(id);
      case "audit_list":
        return ok({ result: "audit_list", value: this.audit })(id);
      case "pair_complete":
        return ok({ result: "ack" })(id);
      case "pair_begin": {
        const pairingId = `PR_${Math.random().toString(36).slice(2, 8)}`;
        this.pairings.set(pairingId, { phase: "awaiting_peer", ticks: 0 });
        const listen = request.params.mode.role === "listen";
        return ok({
          result: "pair_begin",
          value: {
            pairing_id: pairingId,
            listen_addr: listen ? "127.0.0.1:53124" : null,
            code: listen ? "482913" : null,
          },
        })(id);
      }
      case "pair_status": {
        const p = this.pairings.get(request.params.pairing_id);
        if (!p) {
          return {
            request_id: id,
            outcome: { outcome: "err", error: { code: "internal", message: "no such pairing session" } },
          };
        }
        p.ticks += 1;
        if (p.phase === "awaiting_peer" && p.ticks >= 2) p.phase = "awaiting_confirmation";
        if (p.phase === "awaiting_confirmation" && p.accepted === true) {
          p.phase = "done";
          this.emit({ event: "changed", params: { resource: "roster", id: null } });
        }
        if (p.phase === "awaiting_confirmation" && p.accepted === false) p.phase = "failed";
        return ok({
          result: "pair_status",
          value: {
            phase: p.phase,
            phrase: p.phase === "awaiting_confirmation" || p.phase === "done" ? "scotland-revolver-tempest-miracle" : null,
            peer_fingerprint: p.phase === "done" ? "a1b2c3d4e5f6a7b8" : null,
            peer_display_name: p.phase === "done" ? "the-other-laptop" : null,
            error: p.phase === "failed" ? "the other device did not confirm" : null,
          },
        })(id);
      }
      case "pair_confirm": {
        const p = this.pairings.get(request.params.pairing_id);
        if (p) p.accepted = request.params.accept;
        return ok({ result: "ack" })(id);
      }
      case "pair_cancel":
        this.pairings.delete(request.params.pairing_id);
        return ok({ result: "ack" })(id);
      case "export_sealed":
        return ok({ result: "export_sealed", value: { blob_base64: btoa("FERRYSEALEDBLOB\x01...") } })(id);
      case "import_sealed":
        return ok({
          result: "import_sealed",
          value: {
            item_id: "IT_2099",
            origin_peer_id: this.peers[0].peer_id,
            kind: "secret",
            name: "imported-secret.env",
            size_bytes: b(210),
          },
        })(id);
      case "quit":
        return ok({ result: "ack" })(id);
      default:
        return {
          request_id: id,
          outcome: { outcome: "err", error: { code: "internal", message: `mock: unhandled ${request.method}` } },
        };
    }
  }

  onEvent(listener: (event: IpcEvent) => void) {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onPhase(listener: (phase: ConnectionPhase) => void) {
    this.phaseListeners.add(listener);
    return () => this.phaseListeners.delete(listener);
  }
}
