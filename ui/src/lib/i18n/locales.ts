export const LOCALES = ["en", "fr", "de", "es", "pt", "ja", "zh", "ar"] as const;
export type Locale = (typeof LOCALES)[number];
export const DEFAULT_LOCALE: Locale = "en";

// es/pt/ja/zh/ar catalogs stay in the repo (machine-translated, im not a native speaker i cant verify them i could only verify the en/fr/de catalogs)
// but are dormant: not selectable in the UI and not reachable via locale detection.
// Only the shipped ACTIVE_LOCALES are usable. Re-activate by adding one back here.
export const ACTIVE_LOCALES = ["en", "fr", "de"] as const satisfies readonly Locale[];

export const LOCALE_LABELS: Record<Locale, string> = {
  en: "English",
  fr: "Français",
  de: "Deutsch",
  es: "Español",
  pt: "Português (Brasil)",
  ja: "日本語",
  zh: "中文（简体）",
  ar: "العربية",
};

const RTL_LOCALES: ReadonlySet<Locale> = new Set(["ar"]);

export function localeDirection(locale: Locale): "ltr" | "rtl" {
  return RTL_LOCALES.has(locale) ? "rtl" : "ltr";
}

const STORAGE_KEY = "ferry.locale";

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (LOCALES as readonly string[]).includes(value);
}

export function isActiveLocale(value: unknown): value is Locale {
  return typeof value === "string" && (ACTIVE_LOCALES as readonly string[]).includes(value);
}

export function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (isActiveLocale(saved)) return saved;
  } catch {
    void 0;
  }
  const candidates =
    typeof navigator !== "undefined"
      ? [navigator.language, ...(navigator.languages ?? [])]
      : [];
  for (const tag of candidates) {
    const base = tag?.toLowerCase().split("-")[0];
    if (isActiveLocale(base)) return base;
  }
  return DEFAULT_LOCALE;
}

export function persistLocale(locale: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    void 0;
  }
}
