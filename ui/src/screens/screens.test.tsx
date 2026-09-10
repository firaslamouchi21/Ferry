import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { Route, Routes } from "react-router-dom";
import { renderScreen } from "@/test/render";
import { PeersScreen } from "./PeersScreen";
import { InboxScreen } from "./InboxScreen";
import { SendScreen } from "./SendScreen";
import { PairScreen } from "./PairScreen";
import { ActivityScreen } from "./ActivityScreen";
import { MessagesScreen } from "./MessagesScreen";
import { PublishScreen } from "./PublishScreen";
import { TeamRosterScreen } from "./TeamRosterScreen";

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

  it("MessagesScreen lists a conversation and prompts to pick a peer", async () => {
    renderScreen(<MessagesScreen />, { route: "/messages" });
    expect(await screen.findByText("dev-box-02")).toBeTruthy();
    expect(await screen.findByText("yep, opening it now")).toBeTruthy();
  });

  it("a message thread shows both directions in order and Enter sends", async () => {
    renderScreen(
      <Routes>
        <Route path="/messages/:peerId" element={<MessagesScreen />} />
      </Routes>,
      { route: "/messages/a1b2c3d4e5f6071829aabbccddeeff00112233445566778899aabbccddeeff001" },
    );
    const bubble = (t: string) =>
      screen.getAllByText(t).find((el) => el.closest(".bubble")) as HTMLElement;
    await screen.findByText("hey, did the build artifact come through?");
    const incoming = bubble("hey, did the build artifact come through?");
    const outgoing = bubble("yep, opening it now");
    expect(incoming.compareDocumentPosition(outgoing) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    const box = (await screen.findByPlaceholderText(/message/i)) as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: "on it" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(box.value).toBe(""));
  });

  it("PublishScreen and TeamRosterScreen render their not-yet-operating state", async () => {
    renderScreen(<PublishScreen />, { route: "/publish" });
    expect(await screen.findByRole("heading", { name: "Publish" })).toBeTruthy();

    renderScreen(<TeamRosterScreen />, { route: "/team-roster" });
    expect(await screen.findByRole("heading", { name: "Team roster" })).toBeTruthy();
  });
});
