import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { detectHost } from "@/lib/ipc";

export const THEME_PREFS = ["system", "light", "dark"] as const;
export type ThemePref = (typeof THEME_PREFS)[number];

const STORAGE_KEY = "ferry.theme";
const DEFAULT_PREF: ThemePref = "system";

export function isThemePref(value: unknown): value is ThemePref {
  return typeof value === "string" && (THEME_PREFS as readonly string[]).includes(value);
}

function readStoredPref(): ThemePref {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (isThemePref(raw)) return raw;
  } catch {
    void 0;
  }
  return DEFAULT_PREF;
}

function persistPref(pref: ThemePref): void {
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch {
    void 0;
  }
}

function applyPref(pref: ThemePref): void {
  const root = document.documentElement;
  if (pref === "system") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = pref;
  }
  if (pref === "system" && detectHost() === "vscode") {
    root.dataset.host = "vscode";
  } else {
    delete root.dataset.host;
  }
}

interface ThemeValue {
  pref: ThemePref;
  setPref: (next: ThemePref) => void;
}

const ThemeContext = createContext<ThemeValue | null>(null);

export function ThemeProvider({ children, pref: forced }: { children: ReactNode; pref?: ThemePref }) {
  const [pref, setPrefState] = useState<ThemePref>(() => forced ?? readStoredPref());

  useEffect(() => {
    applyPref(pref);
  }, [pref]);

  const setPref = useCallback((next: ThemePref) => {
    setPrefState(next);
    persistPref(next);
  }, []);

  const value = useMemo(() => ({ pref, setPref }), [pref, setPref]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeValue {
  const value = useContext(ThemeContext);
  if (!value) throw new Error("useTheme must be used within a ThemeProvider");
  return value;
}
