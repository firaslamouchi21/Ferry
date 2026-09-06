import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { renderScreen } from "@/test/render";
import { PeersScreen } from "./PeersScreen";
import { InboxScreen } from "./InboxScreen";
import { SendScreen } from "./SendScreen";
import { PairScreen } from "./PairScreen";
import { ActivityScreen } from "./ActivityScreen";

describe("screen smoke tests over the mock daemon", () => {
  it("PeersScreen lists the rostered peers from the daemon", async () => {
    renderScreen(<PeersScreen />, { route: "/peers" });
    expect(await screen.findByText("dev-box-02")).toBeTruthy();
    expect(await screen.findByText("office-linux")).toBeTruthy();
    expect(await screen.findByText("ci-runner-88")).toBeTruthy();
  });

  it("InboxScreen renders the in-flight item and the state tabs", async () => {
    renderScreen(<InboxScreen />, { route: "/inbox" });
    expect(await screen.findByText("build-artifacts.tar.gz")).toBeTruthy();
    expect(await screen.findByRole("tab", { name: /Delivered/ })).toBeTruthy();
  });

  it("SendScreen offers the rostered peers as destinations", async () => {
    renderScreen(<SendScreen />, { route: "/send" });
    expect((await screen.findAllByText(/dev-box-02/)).length).toBeGreaterThan(0);
  });

  it("PairScreen shows the two out-of-band pairing roles", async () => {
    renderScreen(<PairScreen />, { route: "/pair" });
    expect(await screen.findByRole("button", { name: /Show a code/ })).toBeTruthy();
    expect(await screen.findByRole("button", { name: /Enter a code/ })).toBeTruthy();
  });

  it("ActivityScreen renders the content-free audit log header", async () => {
    renderScreen(<ActivityScreen />, { route: "/activity" });
    expect(await screen.findByRole("heading", { name: "Activity" })).toBeTruthy();
  });
});
