// Initials on a color derived from the person's ID, so everyone keeps their color.

function hue(id: string): number {
  let h = 0;
  for (const ch of id.slice(0, 16)) h = (h * 31 + ch.charCodeAt(0)) % 360;
  return h;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const letters = parts.length > 1 ? parts[0][0] + parts[parts.length - 1][0] : name.slice(0, 2);
  return letters.toLocaleUpperCase("tr-TR");
}

interface Props {
  id: string;
  name: string;
  size?: number;
  /** 0..1 voice level; draws a ring that swells while they talk. */
  level?: number;
  online?: boolean;
}

export function Avatar({ id, name, size = 36, level, online }: Props) {
  const ring = level == null ? 0 : Math.min(1, level);
  return (
    <span
      className="avatar"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.38,
        background: `oklch(0.42 0.09 ${hue(id)})`,
        boxShadow: ring > 0.02 ? `0 0 0 ${2 + ring * size * 0.12}px var(--speaking)` : undefined,
      }}
    >
      {initials(name)}
      {online != null && <span className={online ? "presence on" : "presence"} />}
    </span>
  );
}

/** Maps a raw RMS level (speech is roughly 0.01..0.2) onto 0..1 for display. */
export function displayLevel(rms: number): number {
  const db = 20 * Math.log10(Math.max(rms, 1e-5));
  return Math.min(1, Math.max(0, (db + 50) / 40));
}
