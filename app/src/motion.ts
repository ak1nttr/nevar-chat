// Shared pieces for the few things that move: a gate that stops all drawing when nobody can see it,
// a low-rate frame loop, value noise, and the ASCII noise field used by the voice bands.
//
// Rule: nevar runs beside games. Nothing animates while the window is hidden or unfocused, and
// nothing animates more than ~20 times a second.

import { useEffect, useState } from "react";

export const FPS = 20;

const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
const visible = () => document.visibilityState === "visible" && document.hasFocus() && !reduced();

/** True while it's worth animating: the window is shown, focused, and motion isn't turned down. */
export function useLiveMotion(): boolean {
  const [live, setLive] = useState(visible);
  useEffect(() => {
    const update = () => setLive(visible());
    const query = matchMedia("(prefers-reduced-motion: reduce)");
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    query.addEventListener("change", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      query.removeEventListener("change", update);
    };
  }, []);
  return live;
}

/** Calls `draw(dt, t)` about `fps` times a second until the returned stop function runs. */
export function frameLoop(draw: (dt: number, t: number) => void, fps = FPS): () => void {
  let raf = 0;
  let last = 0;
  let acc = 0;
  const frame = (ts: number) => {
    const t = ts / 1000;
    const dt = last ? Math.min(0.1, t - last) : 0;
    last = t;
    acc += dt;
    if (acc >= 1 / fps) {
      draw(acc, t);
      acc = 0;
    }
    raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);
  return () => cancelAnimationFrame(raf);
}

/** Sizes a canvas's backing store to its CSS box and the screen's pixel ratio. */
export function fitCanvas(cv: HTMLCanvasElement) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = cv.clientWidth;
  const h = cv.clientHeight;
  cv.width = Math.max(1, Math.round(w * dpr));
  cv.height = Math.max(1, Math.round(h * dpr));
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { ctx, w, h };
}

// ---------- noise ----------

export function hash3(x: number, y: number, z: number): number {
  let h = (Math.imul(x, 374761393) + Math.imul(y, 668265263) + Math.imul(z, 1274126177)) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export function valueNoise(x: number, y: number, z: number): number {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const zi = Math.floor(z);
  const xf = x - xi;
  const yf = y - yi;
  const zf = z - zi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const w = zf * zf * (3 - 2 * zf);
  const c = (dx: number, dy: number, dz: number) => hash3(xi + dx, yi + dy, zi + dz);
  return lerp(
    lerp(lerp(c(0, 0, 0), c(1, 0, 0), u), lerp(c(0, 1, 0), c(1, 1, 0), u), v),
    lerp(lerp(c(0, 0, 1), c(1, 0, 1), u), lerp(c(0, 1, 1), c(1, 1, 1), u), v),
    w,
  );
}

const fbm = (x: number, y: number, z: number) =>
  valueNoise(x, y, z) * 0.62 + valueNoise(x * 2.03 + 17, y * 2.03 + 9, z * 1.6) * 0.38;

export type Rgb = [number, number, number];

export function hexRgb(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

// ---------- ASCII noise ----------

const BUCKETS = 6;

/**
 * A grid of letters that only shows where a slowly drifting noise field rises above a threshold,
 * so the letters form soft blobs. Louder voice lowers the threshold and speeds the drift up.
 * With `ramp`, the letters are a density ramp instead (light to heavy), picked by brightness.
 */
export class AsciiNoise {
  private ctx: CanvasRenderingContext2D;
  private w: number;
  private h: number;
  private z = Math.random() * 50;
  private lv = 0;

  constructor(
    cv: HTMLCanvasElement,
    private letters: string,
    private colors: [Rgb, Rgb],
    private opts: { cell: number; scale: number; quiet: number; loud: number; ramp?: boolean },
  ) {
    ({ ctx: this.ctx, w: this.w, h: this.h } = fitCanvas(cv));
    this.letters = letters.toLocaleUpperCase("tr").replace(/\s+/g, "") || "•";
  }

  clear() {
    this.ctx.clearRect(0, 0, this.w, this.h);
  }

  /** `level` is 0..1. `still` draws one frame at that level right away, without easing into it. */
  draw(dt: number, level: number, still = false) {
    this.lv = still ? level : this.lv + (level - this.lv) * 0.3;
    const lv = this.lv;
    this.z += dt * (0.1 + lv * 0.9);
    const { ctx } = this;
    const { cell, scale } = this.opts;
    const cols = Math.ceil(this.w / cell);
    const rows = Math.ceil(this.h / cell);
    const th = this.opts.quiet - (this.opts.quiet - this.opts.loud) * lv;
    ctx.clearRect(0, 0, this.w, this.h);
    ctx.font = `${cell}px "Departure Mono", monospace`;
    ctx.textBaseline = "top";
    const buckets: number[][] = Array.from({ length: BUCKETS }, () => []);
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < cols; x++) {
        const n = fbm(x * scale, y * scale * 1.15, this.z);
        if (n <= th) continue;
        const k = Math.min(BUCKETS - 1, Math.floor(((n - th) / (1 - th)) * 2.4 * BUCKETS));
        buckets[k].push(x, y);
      }
    }
    const [lo, hi] = this.colors;
    for (let k = 0; k < BUCKETS; k++) {
      const t = k / (BUCKETS - 1);
      const c = lo.map((v, i) => Math.round(v + (hi[i] - v) * t));
      ctx.fillStyle = `rgba(${c[0]},${c[1]},${c[2]},${(0.35 + 0.65 * t) * (0.75 + 0.25 * lv)})`;
      const b = buckets[k];
      for (let i = 0; i < b.length; i += 2) {
        const x = b[i];
        const y = b[i + 1];
        const ch = this.opts.ramp
          ? this.letters[Math.min(this.letters.length - 1, Math.floor((k / BUCKETS) * this.letters.length))]
          : this.letters[(x + y * 3) % this.letters.length];
        ctx.fillText(ch, x * cell + 1, y * cell);
      }
    }
  }
}
