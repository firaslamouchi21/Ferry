import { describe, expect, it, beforeEach, vi } from "vitest";
import { en } from "./en";
import { fr } from "./fr";
import { de } from "./de";
import { translate } from "./index";
import { detectLocale, persistLocale, isLocale } from "./locales";

function leafKeys(obj: unknown, prefix = ""): string[] {
  if (typeof obj !== "object" || obj === null) return [prefix];
  return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
    leafKeys(v, prefix ? `${prefix}.${k}` : k),
  );
}

describe("catalog completeness", () => {
  const base = leafKeys(en).sort();

  it("fr has exactly the same keys as en", () => {
    expect(leafKeys(fr).sort()).toEqual(base);
  });

  it("de has exactly the same keys as en", () => {
    expect(leafKeys(de).sort()).toEqual(base);
  });

  it("the bulk of French strings diverge from the English source", () => {
    const identical = base.filter(
      (key) => translate("fr", key as never) === translate("en", key as never),
    );
    expect(identical.length / base.length).toBeLessThan(0.1);
  });
});

describe("translate", () => {
  it("interpolates named vars", () => {
    expect(translate("en", "nav.connectedVersion", { version: 7 })).toBe("Connected v7");
  });

  it("leaves unknown placeholders untouched", () => {
    expect(translate("en", "item.crumb")).toBe("Inbox / {id}");
  });

  it("selects the singular plural form for count 1", () => {
    expect(translate("en", "inbox.footer" as never, { count: 1 })).toBe("1 item in your inbox");
  });

  it("selects the plural form for count 4", () => {
    expect(translate("en", "inbox.footer" as never, { count: 4 })).toBe("4 items in your inbox");
  });

  it("falls back to English when a locale lacks the key", () => {
    expect(translate("fr", "does.not.exist" as never)).toBe("does.not.exist");
  });

  it("localises plural rules per locale", () => {
    expect(translate("fr", "peers.footer" as never, { count: 0, reachable: 0 })).toContain("appairé");
  });
});

describe("locale detection", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
    try {
      localStorage.clear();
    } catch {
      void 0;
    }
  });

  it("isLocale accepts known tags only", () => {
    expect(isLocale("fr")).toBe(true);
    expect(isLocale("es")).toBe(true);
    expect(isLocale("tlh")).toBe(false);
    expect(isLocale(null)).toBe(false);
  });

  it("prefers a persisted locale", () => {
    persistLocale("de");
    expect(detectLocale()).toBe("de");
  });

  it("falls back to the navigator language", () => {
    try {
      localStorage.clear();
    } catch {
      void 0;
    }
    vi.stubGlobal("navigator", { language: "fr-FR", languages: ["fr-FR", "en"] });
    expect(detectLocale()).toBe("fr");
  });

  it("maps a regional navigator tag to its base locale", () => {
    try {
      localStorage.clear();
    } catch {
      void 0;
    }
    vi.stubGlobal("navigator", { language: "pt-BR", languages: ["pt-BR"] });
    expect(detectLocale()).toBe("pt");
  });

  it("defaults to English for an unsupported navigator language", () => {
    try {
      localStorage.clear();
    } catch {
      void 0;
    }
    vi.stubGlobal("navigator", { language: "is-IS", languages: ["is-IS"] });
    expect(detectLocale()).toBe("en");
  });
});
