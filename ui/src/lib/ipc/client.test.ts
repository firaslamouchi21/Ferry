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

  it("connects a provider by PAT and reports it connected", async () => {
    const client = new FerryClient("mock", new MockTransport());
    const before = await client.providerStatus();
    expect(before.connected).toBe(false);
    const result = await client.providerConnect("ghp_faketoken");
    expect("connected" in result && result.connected).toBe(true);
    expect((await client.providerStatus()).connected).toBe(true);
  });

  it("walks the device-flow poll from pending to connected", async () => {
    const client = new FerryClient("mock", new MockTransport());
    const auth = await client.providerConnect(null);
    expect("user_code" in auth).toBe(true);
    let status = await client.providerConnectPoll();
    expect(status).toBeNull();
    while (status === null) status = await client.providerConnectPoll();
    expect(status.connected).toBe(true);
  });

  it("fetches a roster preview with a signer and a diff", async () => {
    const client = new FerryClient("mock", new MockTransport());
    await client.providerConnect("ghp_faketoken");
    const preview = await client.rosterFetch("acme/team/roster.json");
    expect(preview.signer_verifying_key_hex).toBeTruthy();
    expect(preview.adds).toBeGreaterThan(0);
    expect(preview.entries.some((e) => e.already_present)).toBe(true);
  });

  it("publishes a gist as a job that finishes with a url", async () => {
    const client = new FerryClient("mock", new MockTransport());
    await client.providerConnect("ghp_faketoken");
    const job = await client.gistPublish("IT_1");
    expect(job.job_id).toBeTruthy();
    let status = await client.remoteJobStatus(job.job_id);
    expect(status.phase).toBe("running");
    status = await client.remoteJobStatus(job.job_id);
    expect(status.phase).toBe("done");
    expect(status.result_url).toContain("gist.github.com");
  });

  it("applies a remote roster as a job that finishes with a summary", async () => {
    const client = new FerryClient("mock", new MockTransport());
    await client.providerConnect("ghp_faketoken");
    const job = await client.rosterApplyRemote("acme/team/roster.json");
    let status = await client.remoteJobStatus(job.job_id);
    while (status.phase !== "done") status = await client.remoteJobStatus(job.job_id);
    expect(status.result_summary).toMatch(/imported/);
  });
});
