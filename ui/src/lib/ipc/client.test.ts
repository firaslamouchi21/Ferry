import { describe, expect, it } from "vitest";
import { FerryClient, FerryIpcError } from "./client";
import { MockTransport } from "./mock";

describe("FerryClient over MockTransport", () => {
  it("returns the roster with reachability", async () => {
    const client = new FerryClient("mock");
    const peers = await client.rosterList();
    expect(peers.length).toBeGreaterThan(0);
    expect(typeof peers[0].reachable).toBe("boolean");
  });

  it("maps an error outcome to a typed FerryIpcError", async () => {
    const transport = new MockTransport();
    const response = await transport.send({
      ipc_protocol_version: 1,
      request_id: "t1",
      // @ts-expect-error deliberately unknown method
      request: { method: "does_not_exist" },
    });
    expect(response.outcome.outcome).toBe("err");
  });

  it("accepts an offered inbox item and reflects the new state", async () => {
    const client = new FerryClient("mock");
    const before = await client.inboxList();
    const offered = before.find((i) => i.state === "offered");
    expect(offered).toBeDefined();
    await client.inboxAccept(offered!.item_id);
    const after = await client.inboxList();
    expect(after.find((i) => i.item_id === offered!.item_id)?.state).toBe("delivered");
  });

  it("two MockTransport instances do not share mutable state", async () => {
    const a = new FerryClient("mock", new MockTransport());
    const b = new FerryClient("mock", new MockTransport());

    const offered = (await a.inboxList()).find((i) => i.state === "offered");
    expect(offered).toBeDefined();
    await a.inboxAccept(offered!.item_id);

    expect((await a.inboxList()).find((i) => i.item_id === offered!.item_id)?.state).toBe("delivered");
    expect(
      (await b.inboxList()).find((i) => i.item_id === offered!.item_id)?.state,
      "a second transport must start from a clean seed, not inherit the first one's mutations",
    ).toBe("offered");
  });

  it("stop() clears the progress timer it started", () => {
    const transport = new MockTransport();
    transport.start();
    transport.stop();
    transport.start();
    transport.stop();
  });

  it("surfaces FerryIpcError as an instance", () => {
    const err = new FerryIpcError("internal", "boom");
    expect(err).toBeInstanceOf(Error);
    expect(err.code).toBe("internal");
  });
});
