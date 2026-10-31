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
}

export function VoiceLane({ label, note, noteTone, levels, sweepAt = 0 }: Props) {
  const bars = levels
    ? levels.map((v) => ({ h: Math.max(3, Math.round(v * 50)), cls: v > TALKING ? "bar live" : "bar" }))
    : Array.from({ length: BARS }, (_, i) => {
        const d = Math.abs(i - (sweepAt % BARS));
        return { h: d === 0 ? 10 : d === 1 ? 6 : 3, cls: d <= 1 ? "bar near" : "bar" };
      });

  return (
    <div>
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
