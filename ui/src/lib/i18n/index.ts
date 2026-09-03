import { createContext, createElement, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { en, type Catalog } from "./en";
import { fr } from "./fr";
import { de } from "./de";
import { DEFAULT_LOCALE, detectLocale, persistLocale, type Locale } from "./locales";

export { LOCALES, LOCALE_LABELS, DEFAULT_LOCALE, isLocale, detectLocale } from "./locales";
export type { Locale } from "./locales";
export type { Catalog } from "./en";

const CATALOGS: Record<Locale, Catalog> = { en, fr, de };

type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${P}${K}`
    : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];

type AllLeaves = Leaves<Catalog>;

type PluralBase<K extends string> = K extends `${infer B}_other` ? B : never;

export type MessageKey = AllLeaves | PluralBase<AllLeaves>;

type Vars = Record<string, string | number>;

function lookup(catalog: Catalog, key: string): string | undefined {
  let node: unknown = catalog;
  for (const part of key.split(".")) {
    if (node && typeof node === "object" && part in node) {
      node = (node as Record<string, unknown>)[part];
    } else {
      return undefined;
    }
  }
  return typeof node === "string" ? node : undefined;
}

function interpolate(template: string, vars?: Vars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name) =>
    name in vars ? String(vars[name]) : whole,
  );
}

export function translate(locale: Locale, key: MessageKey, vars?: Vars): string {
  const catalog = CATALOGS[locale] ?? CATALOGS[DEFAULT_LOCALE];

  if (vars && typeof vars.count === "number") {
    const category = new Intl.PluralRules(locale).select(vars.count);
    const plural =
      lookup(catalog, `${key}_${category}`) ??
      lookup(catalog, `${key}_other`) ??
      lookup(CATALOGS[DEFAULT_LOCALE], `${key}_${category}`) ??
      lookup(CATALOGS[DEFAULT_LOCALE], `${key}_other`);
    if (plural !== undefined) return interpolate(plural, vars);
  }

  const message =
    lookup(catalog, key) ?? lookup(CATALOGS[DEFAULT_LOCALE], key) ?? key;
  return interpolate(message, vars);
}

interface I18nValue {
  locale: Locale;
  setLocale: (next: Locale) => void;
  t: (key: MessageKey, vars?: Vars) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children, locale: forced }: { children: ReactNode; locale?: Locale }) {
  const [locale, setLocaleState] = useState<Locale>(() => forced ?? detectLocale());

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    persistLocale(next);
  }, []);

  useEffect(() => {
    if (typeof document !== "undefined") document.documentElement.lang = locale;
  }, [locale]);

  const t = useCallback((key: MessageKey, vars?: Vars) => translate(locale, key, vars), [locale]);

  const value = useMemo<I18nValue>(() => ({ locale, setLocale, t }), [locale, setLocale, t]);

  return createElement(I18nContext.Provider, { value }, children);
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside <I18nProvider>");
  return value;
}

export function useT(): I18nValue["t"] {
  return useI18n().t;
}

const SPLIT_SENTINEL = "⁣";

export function useTParts(): (key: MessageKey, tokenName: string, vars?: Vars) => string[] {
  const { t } = useI18n();
  return useCallback(
    (key, tokenName, vars) => t(key, { ...vars, [tokenName]: SPLIT_SENTINEL }).split(SPLIT_SENTINEL),
    [t],
  );
}
