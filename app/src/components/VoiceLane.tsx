// The conversation strip: one lane per person, the last few seconds of their voice as bars.

import { useEffect, useRef, useState } from "react";

export const BARS = 44;
/** Above this (on the 0..1 display scale) someone counts as talking. */
export const TALKING = 0.15;

const silence = () => Array<number>(BARS).fill(0);

/** Maps RMS onto 0..1 on a log scale: -50 dBFS is empty, -10 dBFS is full. */
export function displayLevel(rms: number): number {
  const db = 20 * Math.log10(Math.max(rms, 1e-5));
  return Math.min(1, Math.max(0, (db + 50) / 40));
}

/** Samples `value` every 100 ms into a rolling history while `running`. */
export function useHistory(value: number, running: boolean): number[] {
  const latest = useRef(value);
  latest.current = value;
  const [history, setHistory] = useState(silence);

  useEffect(() => {
    setHistory(silence());
    if (!running) return;
    const timer = setInterval(() => setHistory((h) => [...h.slice(1), latest.current]), 100);
    return () => clearInterval(timer);
  }, [running]);

  return history;
}

/** Like `useHistory`, for a changing set of people at once, keyed by ID. Newcomers start silent. */
export function useHistories(levels: Record<string, number>, running: boolean): Record<string, number[]> {
  const latest = useRef(levels);
  latest.current = levels;
  const [histories, setHistories] = useState<Record<string, number[]>>({});

  useEffect(() => {
    setHistories({});
    if (!running) return;
    const timer = setInterval(
      () =>
        setHistories((old) => {
          const next: Record<string, number[]> = {};
          for (const [id, level] of Object.entries(latest.current)) {
            next[id] = [...(old[id] ?? silence()).slice(1), level];
          }
          return next;
        }),
      100,
    );
    return () => clearInterval(timer);
  }, [running]);

  return histories;
}

/** A counter for the waiting sweep while the call isn't connected yet. */
export function useTicker(running: boolean): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => setTick((t) => t + 1), 110);
    return () => clearInterval(timer);
  }, [running]);
  return tick;
}

interface Props {
  label: string;
  note?: string;
  noteTone?: "on" | "warn";
  /** Voice history, 0..1 per bar. Without it the lane shows a waiting sweep at `sweepAt`. */
  levels?: number[];
  sweepAt?: number;
  /** Shorter bars, for calls with several people. */
  compact?: boolean;
}

export function VoiceLane({ label, note, noteTone, levels, sweepAt = 0, compact }: Props) {
  const tall = compact ? 24 : 50;
  const bars = levels
    ? levels.map((v) => ({ h: Math.max(3, Math.round(v * tall)), cls: v > TALKING ? "bar live" : "bar" }))
    : Array.from({ length: BARS }, (_, i) => {
        const d = Math.abs(i - (sweepAt % BARS));
        return { h: d === 0 ? 10 : d === 1 ? 6 : 3, cls: d <= 1 ? "bar near" : "bar" };
      });

  return (
    <div className={compact ? "lane compact" : "lane"}>
      <div className="lane-head">
        <span>{label}</span>
        {note && <span className={noteTone}>{note}</span>}
      </div>
      <div className="bars" aria-hidden="true">
        {bars.map((b, i) => (
          <span key={i} className={b.cls} style={{ height: b.h }} />
        ))}
      </div>
    </div>
  );
}
