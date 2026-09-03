import type { ReactNode } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { ErrorState, Skeleton } from "@/components";
import { useT } from "@/lib/i18n";

export function ScreenHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="screen-header">
      <div>
        <h1>{title}</h1>
        {subtitle ? <p className="screen-subtitle">{subtitle}</p> : null}
      </div>
      {actions ? <div className="screen-actions">{actions}</div> : null}
    </div>
  );
}

export function Async<T>({
  query,
  children,
  skeletonRows,
}: {
  query: UseQueryResult<T>;
  children: (data: T) => ReactNode;
  skeletonRows?: number;
}) {
  const t = useT();
  if (query.isLoading) return <Skeleton rows={skeletonRows} />;
  if (query.isError) return <ErrorState error={query.error} onRetry={() => void query.refetch()} />;
  if (query.data === undefined) return <ErrorState error={new Error(t("common.noData"))} />;
  return <>{children(query.data)}</>;
}

export function Tabs<T extends string>({
  tabs,
  active,
  onChange,
}: {
  tabs: { id: T; label: string; count?: number }[];
  active: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="tabs" role="tablist">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={active === t.id}
          className={`tab ${active === t.id ? "active" : ""}`}
          onClick={() => onChange(t.id)}
        >
          {t.label}
          {t.count != null ? <span className="tab-count">{t.count}</span> : null}
        </button>
      ))}
    </div>
  );
}
