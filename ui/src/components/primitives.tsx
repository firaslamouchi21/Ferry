import { useEffect, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Check, Copy, Eye, EyeOff } from "lucide-react";
import type { TransferState } from "@/lib/ipc";
import { useFormat } from "@/lib/format";
import { useT } from "@/lib/i18n";

type Variant = "primary" | "ghost" | "danger";

export function Button({
  variant = "ghost",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return <button {...props} className={`btn btn-${variant} ${className}`.trim()} />;
}

export function StatusChip({ tone, children }: { tone: "online" | "offline" | "warn" | "error" | "neutral"; children: ReactNode }) {
  return <span className={`chip chip-${tone}`}>{children}</span>;
}

const STATE_TONE: Record<TransferState, "online" | "offline" | "warn" | "error" | "neutral"> = {
  queued: "neutral",
  offered: "warn",
  accepted: "warn",
  transferring: "warn",
  delivered: "online",
  opened: "online",
  expired: "error",
  failed: "error",
};

export function StateBadge({ state }: { state: TransferState }) {
  return <StatusChip tone={STATE_TONE[state]}>{state}</StatusChip>;
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </label>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} className="toggle" onClick={() => onChange(!checked)}>
      <span className={`toggle-track ${checked ? "on" : ""}`}>
        <span className="toggle-thumb" />
      </span>
      <span>{label}</span>
    </button>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="state-panel">
      <p className="state-title">{title}</p>
      {children ? <div className="state-body">{children}</div> : null}
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const t = useT();
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="state-panel">
      <p className="state-title">{t("common.somethingWentWrong")}</p>
      <div className="state-body mono">{message}</div>
      {onRetry ? (
        <Button variant="ghost" onClick={onRetry}>
          {t("common.retry")}
        </Button>
      ) : null}
    </div>
  );
}

export function Skeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="skeleton" aria-hidden>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="skeleton-row" />
      ))}
    </div>
  );
}

export function Fingerprint({ value }: { value: string }) {
  return <span className="mono fingerprint">{value}</span>;
}

export function RelativeTime({ millis }: { millis: number | null | undefined }) {
  const fmt = useFormat();
  return <span className="relative-time">{fmt.relative(millis)}</span>;
}

export function ProgressBar({ value, total }: { value: number; total: number }) {
  const pct = total > 0 ? Math.min(100, Math.round((value / total) * 100)) : 0;
  return (
    <div className="progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
      <div className="progress-fill" style={{ width: `${pct}%` }} />
      <span className="progress-label mono">{pct}%</span>
    </div>
  );
}

async function clearClipboardIfUnchanged(text: string): Promise<void> {
  try {
    if ((await navigator.clipboard.readText()) === text) {
      await navigator.clipboard.writeText("");
    }
  } catch {
    void 0;
  }
}

export function CopyButton({
  text,
  label,
  sensitive = false,
}: {
  text: string;
  label?: string;
  sensitive?: boolean;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const timer = useRef<number>();
  const resolvedLabel = label ?? t("common.copy");

  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <Button
      variant="ghost"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          window.clearTimeout(timer.current);
          timer.current = window.setTimeout(
            () => {
              setCopied(false);
              if (sensitive) void clearClipboardIfUnchanged(text);
            },
            sensitive ? 20_000 : 2_000,
          );
        } catch {
          setCopied(false);
        }
      }}
      title={copied && sensitive ? t("common.clipboardCleared") : resolvedLabel}
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
      {copied ? t("common.copied") : resolvedLabel}
    </Button>
  );
}

export function MaskedValue({ value }: { value: string }) {
  const t = useT();
  const [revealed, setRevealed] = useState(false);
  const timer = useRef<number>();
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (
    <span
      className="masked"
      onBlur={() => setRevealed(false)}
      tabIndex={-1}
    >
      <span className="mono masked-text">{revealed ? value : "•".repeat(Math.min(24, Math.max(6, value.length)))}</span>
      <button
        type="button"
        className="masked-toggle"
        aria-label={revealed ? t("common.hideValue") : t("common.revealValue")}
        onClick={() => {
          setRevealed((r) => !r);
          window.clearTimeout(timer.current);
          if (!revealed) timer.current = window.setTimeout(() => setRevealed(false), 15_000);
        }}
      >
        {revealed ? <EyeOff size={13} /> : <Eye size={13} />}
      </button>
    </span>
  );
}
