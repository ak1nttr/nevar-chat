// A thin ASCII strip, made of a person's name, that flows while they talk. Silent, it's empty and
// costs nothing: the frame loop only runs while there's voice (and briefly after, to fade out).

import { useEffect, useRef, useState } from "react";
import { AsciiNoise, frameLoop, hexRgb, Rgb, useLiveMotion } from "../motion";
import { Shade, useShade } from "../theme";

const LEVEL_ON = 0.02;

type Look = "band" | "strip";

/** Letter colors, from dim to bright. "band" sits on the page, "strip" on the accent-blue call strip. */
const COLORS: Record<Look, Record<Shade, [Rgb, Rgb]>> = {
  band: { light: [hexRgb("#AFC0FF"), hexRgb("#0A3CFF")], dark: [hexRgb("#22305F"), hexRgb("#8EA8FF")] },
  // Kept a step below white, so the names and timer on top stay readable.
  strip: { light: [hexRgb("#3A63FF"), hexRgb("#9FB4FF")], dark: [hexRgb("#4F75FF"), hexRgb("#A9BCFF")] },
};

const SHAPE: Record<Look, { cell: number; scale: number; quiet: number; loud: number }> = {
  band: { cell: 9, scale: 0.16, quiet: 0.66, loud: 0.36 },
  strip: { cell: 9, scale: 0.13, quiet: 0.6, loud: 0.42 },
};

interface Props {
  letters: string;
  /** 0..1 on the display scale. */
  level: number;
  look?: Look;
  className?: string;
}

export function VoiceBand({ letters, level, look = "band", className = "band" }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const latest = useRef(level);
  latest.current = level;
  const shade = useShade();
  const live = useLiveMotion();

  // Keep drawing ~0.7 s after the voice stops, so it fades instead of vanishing.
  const loud = level > LEVEL_ON;
  const [running, setRunning] = useState(loud);
  useEffect(() => {
    if (loud) {
      setRunning(true);
      return;
    }
    const timer = setTimeout(() => setRunning(false), 700);
    return () => clearTimeout(timer);
  }, [loud]);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const field = new AsciiNoise(cv, letters, COLORS[look][shade], SHAPE[look]);
    if (!running) {
      field.clear();
      return;
    }
    // A frame right away; then, only while the window is seen, keep it flowing.
    field.draw(1, latest.current, true);
    if (!live) return;
    return frameLoop((dt) => field.draw(dt, latest.current));
  }, [running, live, letters, look, shade]);

  return <canvas ref={ref} className={className} aria-hidden="true" />;
}
