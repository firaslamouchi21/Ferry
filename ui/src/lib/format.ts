import { useMemo } from "react";
import { useI18n, type Locale } from "@/lib/i18n";

const BYTE_UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"] as const;

export function formatBytes(bytes: number, locale: Locale = "en"): string {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : value < 10 ? 1 : 0;
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: BYTE_UNITS[unit],
    unitDisplay: "short",
    maximumFractionDigits: digits,
  }).format(value);
}

const RELATIVE_STEPS: [ms: number, unit: Intl.RelativeTimeFormatUnit][] = [
  [60_000, "second"],
  [3_600_000, "minute"],
  [86_400_000, "hour"],
  [Infinity, "day"],
];

export function formatRelative(millis: number | null | undefined, locale: Locale = "en"): string {
  if (millis == null) return "—";
  const delta = millis - Date.now();
  const abs = Math.abs(delta);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (abs < 15_000) return rtf.format(0, "second");
  if (abs < 60_000) return rtf.format(Math.round(delta / 1000), "second");
  for (const [ceiling, unit] of RELATIVE_STEPS) {
    if (abs < ceiling) {
      const divisor =
        unit === "second" ? 1000 : unit === "minute" ? 60_000 : unit === "hour" ? 3_600_000 : 86_400_000;
      return rtf.format(Math.round(delta / divisor), unit);
    }
  }
  return rtf.format(Math.round(delta / 86_400_000), "day");
}

export function formatClock(millis: number | null | undefined, locale?: string): string {
  if (millis == null) return "—";
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
    timeZoneName: "short",
  }).format(new Date(millis));
}

export function formatTime(millis: number | null | undefined, locale?: string): string {
  if (millis == null) return "—";
  return new Intl.DateTimeFormat(locale, { timeStyle: "medium" }).format(new Date(millis));
}

export function useFormat() {
  const { locale } = useI18n();
  return useMemo(
    () => ({
      bytes: (n: number) => formatBytes(n, locale),
      relative: (m: number | null | undefined) => formatRelative(m, locale),
      clock: (m: number | null | undefined) => formatClock(m, locale),
      time: (m: number | null | undefined) => formatTime(m, locale),
    }),
    [locale],
  );
}

const DECODER = typeof TextDecoder !== "undefined" ? new TextDecoder() : null;

export function decodeBase64(b64: string): string {
  const binary = atob(b64);
  if (!DECODER) return binary;
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return DECODER.decode(bytes);
}

export function encodeBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
