import type { ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { Route, Routes } from "react-router-dom";
import { renderScreen } from "@/test/render";
import { FerryClient, MockTransport, type IpcEnvelope, type IpcResponse, type PickedFile } from "@/lib/ipc";
import { SendScreen } from "./SendScreen";
import { ItemDetailScreen } from "./ItemDetailScreen";

class RecordingTransport extends MockTransport {
  requests: IpcEnvelope["request"][] = [];
  picked: PickedFile | null = null;
  saved: { name: string; bytes: Uint8Array }[] = [];
  pickSavePath?: (name: string) => Promise<PickedFile | null>;

  override send(envelope: IpcEnvelope): Promise<IpcResponse> {
    this.requests.push(envelope.request);
    return super.send(envelope).then((response) => {
      if (response.outcome.outcome === "ok" && response.outcome.value.result === "inbox_list") {
        response.outcome.value.value.push({
          item_id: "IT_FILE",
          origin_peer_id: "a1b2c3d4e5f6071829aabbccddeeff00112233445566778899aabbccddeeff001",
          origin_display_name: "dev-box-02",
          kind: "file",
          name: "diagram.png",
          size_bytes: BigInt(12),
          state: "delivered",
          is_burn_after_read: false,
          delivered_at_millis: BigInt(Date.now()),
          expires_at_millis: null,
          opened_at_millis: null,
        } as never);
      }
      return response;
    });
  }

  override pickFile(): Promise<PickedFile | null> {
    return Promise.resolve(this.picked);
  }

  override async saveFile(name: string, produce: () => Promise<Uint8Array>): Promise<boolean> {
    this.saved.push({ name, bytes: await produce() });
    return true;
  }
}

function setup(route: string, node: ReactElement) {
  const transport = new RecordingTransport();
  const client = new FerryClient("mock", transport);
  renderScreen(node, { route, client });
  return transport;
}

describe("file transfer through the UI", () => {
  it("a browser-picked file is sent inline as bytes, never as a bare filename path", async () => {
    const transport = setup("/send", <SendScreen />);
    await screen.findAllByText(/dev-box-02/);

    const input = document.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 1, 2, 3])], "diagram.png");
    fireEvent.change(input, { target: { files: [file] } });
    expect((await screen.findAllByText("diagram.png")).length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: /Execute transfer/ }));

    await waitFor(() => {
      const req = transport.requests.find((r) => r.method === "send_inline");
      expect(req).toBeTruthy();
    });
    const req = transport.requests.find((r) => r.method === "send_inline")!;
    expect(req.method === "send_inline" && req.params.kind).toBe("file");
    expect(req.method === "send_inline" && req.params.name).toBe("diagram.png");
    expect(req.method === "send_inline" && atob(req.params.content_base64).length).toBe(8);
    expect(transport.requests.some((r) => r.method === "send")).toBe(false);
  });

  it("a host-picked file is sent by its real path", async () => {
    const transport = setup("/send", <SendScreen />);
    transport.picked = { path: "/home/me/report.pdf", name: "report.pdf" };
    await screen.findAllByText(/dev-box-02/);

    fireEvent.click(screen.getByText("Choose a file"));
    expect((await screen.findAllByText("report.pdf")).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: /Execute transfer/ }));

    await waitFor(() => expect(transport.requests.some((r) => r.method === "send")).toBe(true));
    const req = transport.requests.find((r) => r.method === "send")!;
    expect(req.method === "send" && req.params.source_path).toBe("/home/me/report.pdf");
    expect(req.method === "send" && req.params.name).toBe("report.pdf");
  });

  it("a route-supplied path (explorer → Send File) is used as the source path", async () => {
    const transport = setup("/send?path=%2Fsrv%2Fapp%2F.env", <SendScreen />);
    await screen.findAllByText(/dev-box-02/);
    expect((await screen.findAllByText(".env")).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: /Execute transfer/ }));
    await waitFor(() => expect(transport.requests.some((r) => r.method === "send")).toBe(true));
    const req = transport.requests.find((r) => r.method === "send")!;
    expect(req.method === "send" && req.params.source_path).toBe("/srv/app/.env");
  });

  function openItem() {
    return setup(
      "/inbox/IT_FILE",
      <Routes>
        <Route path="/inbox/:itemId" element={<ItemDetailScreen />} />
      </Routes>,
    );
  }

  const methods = (transport: RecordingTransport) => transport.requests.map((r) => r.method);

  it("a host that can pick a destination has the daemon write the file itself", async () => {
    const transport = openItem();
    transport.pickSavePath = () => Promise.resolve({ path: "/home/me/diagram.png", name: "diagram.png" });
    fireEvent.click(await screen.findByRole("button", { name: "Save file" }));

    expect(await screen.findByText("Saved to diagram.png.")).toBeTruthy();
    const req = transport.requests.find((r) => r.method === "save_to")!;
    expect(req.method === "save_to" && req.params.destination).toBe("/home/me/diagram.png");
    expect(methods(transport)).not.toContain("open");
    expect(methods(transport)).not.toContain("confirm_opened");
  });

  it("cancelling the destination dialog leaves the item untouched", async () => {
    const transport = openItem();
    transport.pickSavePath = () => Promise.resolve(null);
    fireEvent.click(await screen.findByRole("button", { name: "Save file" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save file" })).toHaveProperty("disabled", false));
    expect(methods(transport)).not.toContain("save_to");
    expect(methods(transport)).not.toContain("confirm_opened");
  });

  it("a browser host downloads the bytes, then marks the item opened", async () => {
    const transport = openItem();
    fireEvent.click(await screen.findByRole("button", { name: "Save file" }));

    await waitFor(() => expect(transport.saved).toHaveLength(1));
    expect(new TextDecoder().decode(transport.saved[0].bytes)).toContain("POSTGRES_URL");
    expect(await screen.findByText("Saved to diagram.png.")).toBeTruthy();
    await waitFor(() => expect(methods(transport)).toContain("confirm_opened"));
    expect(document.querySelector("pre.payload-pre")).toBeNull();
  });

  it("a failed save reports why and never marks the item opened, so nothing is burned", async () => {
    const transport = openItem();
    vi.spyOn(transport, "saveFile").mockImplementation(async (_name, produce) => {
      await produce();
      throw new Error("disk full");
    });
    fireEvent.click(await screen.findByRole("button", { name: "Save file" }));
    expect(await screen.findByText(/disk full/)).toBeTruthy();
    expect(methods(transport)).toContain("open");
    expect(methods(transport)).not.toContain("confirm_opened");
  });
});
