// Segmented level meter, like the LED ladder on a mixing desk.

interface Props {
  /** Raw RMS level of the audio (speech is roughly 0.01..0.2). */
  level: number;
  segments?: number;
  /** Dims the whole meter, e.g. while muted. */
  off?: boolean;
  className?: string;
}

export function Meter({ level, segments = 12, off, className }: Props) {
  const lit = off ? 0 : Math.round(displayLevel(level) * segments);
  return (
    <span className={`meter ${className ?? ""}`} aria-hidden="true">
      {Array.from({ length: segments }, (_, i) => (
        <span key={i} className={i < lit ? (i >= segments - 2 ? "seg hot" : "seg on") : "seg"} />
      ))}
    </span>
  );
}

/** Maps RMS onto 0..1 on a log scale: -50 dBFS is empty, -10 dBFS is full. */
export function displayLevel(rms: number): number {
  const db = 20 * Math.log10(Math.max(rms, 1e-5));
  return Math.min(1, Math.max(0, (db + 50) / 40));
}
