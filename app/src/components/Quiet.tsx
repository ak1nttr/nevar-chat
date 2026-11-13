// The empty state: when nobody is around, a small field of ASCII weather drifts in a box, with one
// line saying so. Slow (12 fps), and still whenever the window isn't seen.

import { useEffect, useRef } from "react";
import { AsciiNoise, frameLoop, hexRgb, Rgb, useLiveMotion } from "../motion";
import { Shade, useShade } from "../theme";

const RAMP = "·:-=+*#";

const COLORS: Record<Shade, [Rgb, Rgb]> = {
  light: [hexRgb("#C4C2BA"), hexRgb("#6B6963")],
  dark: [hexRgb("#36363B"), hexRgb("#8E8C86")],
};

export function Quiet({ children }: { children: React.ReactNode }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const shade = useShade();
  const live = useLiveMotion();

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const field = new AsciiNoise(cv, RAMP, COLORS[shade], { cell: 11, scale: 0.09, quiet: 0.42, loud: 0.42, ramp: true });
    field.draw(1, 0, true);
    if (!live) return;
    return frameLoop((dt) => field.draw(dt * 0.6, 0), 12);
  }, [shade, live]);

  return (
    <div className="quiet glass">
      <canvas ref={ref} className="quiet-field" aria-hidden="true" />
      <p className="quiet-text">{children}</p>
    </div>
  );
}
