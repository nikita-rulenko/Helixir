import { useCallback, useEffect, useState } from "react";
import { apiGet } from "../api";

export function useResource<T>(path: string, { pollMs = 0 }: { pollMs?: number } = {}) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const load = useCallback((background: boolean) => {
    if (!background) setLoading(true);
    setError(null);
    apiGet<T>(path).then(setData).catch(reason => setError(reason instanceof Error ? reason.message : "Request failed")).finally(() => setLoading(false));
  }, [path]);
  const refresh = useCallback(() => load(false), [load]);
  useEffect(refresh, [refresh]);
  useEffect(() => {
    if (!pollMs) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") load(true);
    }, pollMs);
    return () => window.clearInterval(timer);
  }, [load, pollMs]);
  return { data, error, loading, refresh };
}

export function PageState({ loading, error }: { loading: boolean; error: string | null }) {
  if (loading) return <div className="page-state"><i /><strong>Reading the living graph…</strong></div>;
  if (error) return <div className="error-ribbon" role="alert"><strong>Projection unavailable.</strong> {error}</div>;
  return null;
}

export function relativeAge(seconds: number | null) {
  if (seconds === null) return "never";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
