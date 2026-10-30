// A square initials tile, tinted per person so everyone keeps their color.

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
  online?: boolean;
}

export function Avatar({ id, name, size = 32, online }: Props) {
  const h = hue(id);
  return (
    <span
      className="avatar"
      style={{
        width: size,
        height: size,
        fontSize: Math.round(size * 0.36),
        borderRadius: Math.max(6, Math.round(size * 0.22)),
        color: `oklch(0.86 0.11 ${h})`,
        background: `oklch(0.27 0.045 ${h})`,
      }}
    >
      {initials(name)}
      {online != null && <span className={online ? "presence on" : "presence"} />}
    </span>
  );
}
