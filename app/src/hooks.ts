import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, AppState, CallStats, Notice, describeNotice } from "./api";

/** The backend's state snapshot, refetched whenever it says something changed. */
export function useAppState(): AppState | null {
  const [state, setState] = useState<AppState | null>(null);

  useEffect(() => {
    let alive = true;
    let pending = false;
    const refresh = () => {
      // Coalesce bursts of change events into one fetch.
      if (pending) return;
      pending = true;
      queueMicrotask(() => {
        pending = false;
        api.getState().then((s) => alive && setState(s));
      });
    };
    refresh();
    const unlisten = listen("state-changed", refresh);
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, []);

  return state;
}

/** Live call numbers, polled while a call is active. */
export function useCallStats(active: boolean): CallStats | null {
  const [stats, setStats] = useState<CallStats | null>(null);
  useEffect(() => {
    if (!active) {
      setStats(null);
      return;
    }
    let alive = true;
    const tick = () => api.getCallStats().then((s) => alive && setStats(s));
    tick();
    const timer = setInterval(tick, 100);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [active]);
  return stats;
}

export interface Toast {
  id: number;
  text: string;
}

/**
 * Short-lived messages: backend notices plus anything the UI reports.
 * `onNotice` sees each backend notice first and returns false to keep it out of the toasts.
 */
export function useToasts(onNotice?: (n: Notice) => boolean) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(0);
  const filter = useRef(onNotice);
  filter.current = onNotice;

  const push = useCallback((text: string) => {
    const id = nextId.current++;
    setToasts((t) => [...t, { id, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 5000);
  }, []);

  useEffect(() => {
    const unlisten = listen<Notice>("notice", (e) => {
      if (filter.current?.(e.payload) ?? true) push(describeNotice(e.payload));
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [push]);

  return { toasts, push };
}

/** Seconds since `sinceMs`, ticking once a second. */
export function useElapsed(sinceMs: number | null): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (sinceMs == null) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [sinceMs]);
  return sinceMs == null ? 0 : Math.max(0, Math.floor((now - sinceMs) / 1000));
}
