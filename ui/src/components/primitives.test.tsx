import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { I18nProvider } from "@/lib/i18n";
import { CopyButton } from "./primitives";

function renderWithI18n(node: ReactElement) {
  return render(<I18nProvider locale="en">{node}</I18nProvider>);
}

async function clickAndFlush() {
  await act(async () => {
    screen.getByRole("button").click();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("CopyButton", () => {
  let clip: { text: string; writeText: ReturnType<typeof vi.fn>; readText: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    vi.useFakeTimers();
    clip = {
      text: "",
      writeText: vi.fn(async (v: string) => {
        clip.text = v;
      }),
      readText: vi.fn(async () => clip.text),
    };
    vi.stubGlobal("navigator", { clipboard: clip });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("copies the text on click", async () => {
    renderWithI18n(<CopyButton text="fingerprint-abc" />);
    await clickAndFlush();
    expect(clip.writeText).toHaveBeenCalledWith("fingerprint-abc");
  });

  it("does not touch the clipboard again for a non-sensitive copy", async () => {
    renderWithI18n(<CopyButton text="fingerprint-abc" />);
    await clickAndFlush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(clip.readText).not.toHaveBeenCalled();
    expect(clip.text).toBe("fingerprint-abc");
  });

  it("clears a sensitive copy from the clipboard after the timeout", async () => {
    renderWithI18n(<CopyButton text="s3cr3t" sensitive />);
    await clickAndFlush();
    expect(clip.text).toBe("s3cr3t");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(clip.readText).toHaveBeenCalled();
    expect(clip.text).toBe("");
  });

  it("does not overwrite the clipboard if the user copied something else", async () => {
    renderWithI18n(<CopyButton text="s3cr3t" sensitive />);
    await clickAndFlush();
    clip.text = "user copied this later";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(clip.text).toBe("user copied this later");
  });
});
