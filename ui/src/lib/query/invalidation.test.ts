import { describe, expect, it } from "vitest";
import { FerryClient, MockTransport, type IpcEvent, type IpcResource } from "@/lib/ipc";
import { queryKeys, resourceInvalidations } from "./keys";

const ALL_RESOURCES: IpcResource[] = ["transfer", "message", "peer", "roster", "audit"];

describe("event → query invalidation mapping", () => {
  it("maps every IpcResource the daemon can emit", () => {
    for (const resource of ALL_RESOURCES) {
      const keys = resourceInvalidations[resource];
      expect(keys, `resource "${resource}" has no invalidation targets`).toBeDefined();
      expect(keys.length).toBeGreaterThan(0);
    }
    expect(Object.keys(resourceInvalidations).sort()).toEqual([...ALL_RESOURCES].sort());
  });

  it("invalidates the lists a transfer change can affect", () => {
    const keys = resourceInvalidations.transfer.map((k) => k.join("/"));
    expect(keys).toContain(queryKeys.inbox.join("/"));
    expect(keys).toContain(queryKeys.sent.join("/"));
    expect(keys).toContain(queryKeys.audit.join("/"));
  });

  it("a roster change also invalidates the signed-roster export", () => {
    const keys = resourceInvalidations.roster.map((k) => k.join("/"));
    expect(keys).toContain(queryKeys.roster.join("/"));
    expect(keys).toContain(queryKeys.rosterExport.join("/"));
  });

  it("a peer change does not needlessly invalidate transfer lists", () => {
    const keys = resourceInvalidations.peer.map((k) => k.join("/"));
    expect(keys).not.toContain(queryKeys.inbox.join("/"));
    expect(keys).not.toContain(queryKeys.sent.join("/"));
  });
});

describe("daemon events reach the client's listeners", () => {
  it("a state-changing request emits a changed event carrying the affected item", async () => {
    const transport = new MockTransport();
    const client = new FerryClient("mock", transport);
    const seen: IpcEvent[] = [];
    const off = client.onEvent((event) => seen.push(event));

    const inbox = await client.inboxList();
    const offered = inbox.find((i) => i.state === "offered");
    expect(offered).toBeDefined();
    await client.inboxAccept(offered!.item_id);

    off();
    const changed = seen.filter((e) => e.event === "changed");
    expect(changed.length).toBeGreaterThan(0);
    expect(changed.some((e) => e.event === "changed" && e.params.id === offered!.item_id)).toBe(true);
  });

  it("stops delivering events after the listener is detached", () => {
    const transport = new MockTransport();
    const client = new FerryClient("mock", transport);
    const seen: IpcEvent[] = [];
    const off = client.onEvent((event) => seen.push(event));

    transport.emit({ event: "changed", params: { resource: "roster", id: null } });
    expect(seen).toHaveLength(1);

    off();
    transport.emit({ event: "changed", params: { resource: "roster", id: null } });
    expect(seen).toHaveLength(1);
  });
});
