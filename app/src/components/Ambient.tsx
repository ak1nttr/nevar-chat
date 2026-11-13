// The home page's background: a soft dithered halo behind the title, and a few pixel sharks seen
// from above, slowly swimming through. Drawn on one canvas at ~20 fps, only while the window is
// visible and focused; otherwise it holds still. Turned off in settings, only the halo stays.

import { useEffect, useRef } from "react";
import { fitCanvas, frameLoop, hash3, useLiveMotion } from "../motion";
import { Shade, useShade } from "../theme";

const CELL = 8;
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

const PALETTE: Record<Shade, { buckets: [string, string][]; speck: string; halo: string }> = {
  light: {
    buckets: [["#E7E5DF", "#E3E6E8"], ["#DAD8D0", "#D3D9DE"], ["#CCCAC2", "#C3CBD2"], ["#BDBEB8", "#B1BBC4"]],
    speck: "120,128,136",
    halo: "rgba(17,17,16,0.22)",
  },
  dark: {
    buckets: [["#1C1D20", "#1B1E21"], ["#222428", "#21262B"], ["#292C31", "#272E37"], ["#31353A", "#2D3743"]],
    speck: "140,150,165",
    halo: "rgba(237,237,234,0.15)",
  },
};

interface Fish {
  x: number;
  y: number;
  a: number;
  v: number;
  s: number;
  phase: number;
  seed: number;
  last: number;
  tail?: [number, number];
}

interface Speck {
  x: number;
  y: number;
  life: number;
}

class Sea {
  private ctx: CanvasRenderingContext2D;
  private w: number;
  private h: number;
  private cols: number;
  private rows: number;
  private buf: Float32Array;
  private halo: HTMLCanvasElement;
  private specks: Speck[] = [];

  constructor(
    private cv: HTMLCanvasElement,
    private shade: Shade,
    private fish: Fish[],
    haloAt: [number, number],
  ) {
    ({ ctx: this.ctx, w: this.w, h: this.h } = fitCanvas(cv));
    this.cols = Math.ceil(this.w / CELL);
    this.rows = Math.ceil(this.h / CELL);
    this.buf = new Float32Array(this.cols * this.rows);
    this.halo = this.drawHalo(haloAt[0], haloAt[1], 190);
  }

  /** An ordered (Bayer) dither of a radial falloff, rendered once. */
  private drawHalo(cx: number, cy: number, r: number): HTMLCanvasElement {
    const off = document.createElement("canvas");
    off.width = this.cv.width;
    off.height = this.cv.height;
    const g = off.getContext("2d")!;
    const dpr = this.cv.width / Math.max(1, this.w);
    g.scale(dpr, dpr);
    g.fillStyle = PALETTE[this.shade].halo;
    const p = 3;
    for (let y = 0; y < this.h; y += p) {
      for (let x = 0; x < this.w; x += p) {
        const d = Math.hypot((x - cx) / 1.25, y - cy) / r;
        if (d >= 1) continue;
        const v = (1 - d) ** 1.8 * 0.85;
        if (v > (BAYER[((y / p) & 3) * 4 + ((x / p) & 3)] + 0.5) / 16) g.fillRect(x, y, 1.5, 1.5);
      }
    }
    return off;
  }

  step(dt: number, t: number) {
    const { w, h } = this;
    for (const f of this.fish) {
      f.phase += dt * 2.4;
      f.a += Math.sin(t * 0.09 + f.seed) * 0.16 * dt;
      f.x += Math.cos(f.a) * f.v * dt;
      f.y += Math.sin(f.a) * f.v * dt;
      // Swim off one edge, come back on the other.
      const m = 180 * f.s;
      if (f.x < -m) f.x = w + m * 0.6;
      if (f.x > w + m) f.x = -m * 0.6;
      if (f.y < -m) f.y = h + m * 0.6;
      if (f.y > h + m) f.y = -m * 0.6;
      if (f.tail && t - f.last > 0.45) {
        f.last = t;
        this.specks.push({ x: f.tail[0] + (Math.random() - 0.5) * 10, y: f.tail[1] + (Math.random() - 0.5) * 10, life: 1 });
      }
    }
    for (const s of this.specks) s.life -= dt / 4;
    this.specks = this.specks.filter((s) => s.life > 0);
  }

  private stamp(px: number, py: number, r: number, val: number) {
    const { cols, rows, buf } = this;
    const x0 = Math.max(0, Math.floor((px - r) / CELL));
    const x1 = Math.min(cols - 1, Math.floor((px + r) / CELL));
    const y0 = Math.max(0, Math.floor((py - r) / CELL));
    const y1 = Math.min(rows - 1, Math.floor((py + r) / CELL));
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const d = Math.hypot(x * CELL + CELL / 2 - px, y * CELL + CELL / 2 - py);
        if (d >= r) continue;
        const v = val * (1 - (d / r) * 0.65);
        const k = y * cols + x;
        if (v > buf[k]) buf[k] = v;
      }
    }
  }

  draw(sharks: boolean) {
    const { ctx, w, h, cols, buf } = this;
    ctx.clearRect(0, 0, w, h);
    ctx.drawImage(this.halo, 0, 0, w, h);
    if (!sharks) return;

    buf.fill(0);
    for (const f of this.fish) {
      const N = 26;
      const seg = 6.2 * f.s;
      const R = 17 * f.s;
      let x = f.x;
      let y = f.y;
      const back = f.a + Math.PI;
      const spine: [number, number, number][] = [];
      for (let i = 0; i < N; i++) {
        const t = i / (N - 1);
        // The body bends more toward the tail.
        const ang = back + Math.sin(f.phase - i * 0.42) * 0.55 * t ** 1.4;
        spine.push([x, y, ang]);
        x += Math.cos(ang) * seg;
        y += Math.sin(ang) * seg;
      }
      spine.forEach(([px, py], i) => {
        const t = i / (N - 1);
        const r = R * (t < 0.14 ? 0.55 + t * 3.2 : t < 0.34 ? 1 : 1 - ((t - 0.34) / 0.66) * 0.86);
        this.stamp(px, py, r, 1 - t * 0.35);
      });
      const fins = (i: number, off: number, rr: number, v: number) => {
        const [px, py, a] = spine[i];
        const nx = -Math.sin(a);
        const ny = Math.cos(a);
        this.stamp(px + nx * off, py + ny * off, rr, v);
        this.stamp(px - nx * off, py - ny * off, rr, v);
      };
      fins(8, R * 1.25, R * 0.48, 0.7);
      fins(9, R * 1.55, R * 0.36, 0.55);
      fins(N - 1, R * 0.75, R * 0.42, 0.6);
      fins(N - 2, R * 0.5, R * 0.38, 0.6);
      f.tail = [spine[N - 1][0], spine[N - 1][1]];
    }

    const pal = PALETTE[this.shade];
    for (let k = 0; k < buf.length; k++) {
      const v = buf[k];
      if (!v) continue;
      const cx = k % cols;
      const cy = (k / cols) | 0;
      ctx.fillStyle = pal.buckets[Math.min(3, Math.floor(v * 4))][(hash3(cx, cy, 7) * 2) | 0];
      ctx.fillRect(cx * CELL + 0.5, cy * CELL + 0.5, CELL - 1, CELL - 1);
    }
    for (const s of this.specks) {
      ctx.fillStyle = `rgba(${pal.speck},${s.life * 0.45})`;
      ctx.fillRect(Math.round(s.x / 2) * 2, Math.round(s.y / 2) * 2, 2, 2);
    }
  }
}

function startingFish(): Fish[] {
  return [0, 1, 2].map((i) => ({
    x: 60 + i * 120,
    y: 120 + i * 190,
    a: [0.3, 2.6, -0.4][i],
    v: 13 + i * 3,
    s: [1, 0.75, 0.9][i],
    phase: i * 2,
    seed: i * 11.3,
    last: 0,
  }));
}

export function Ambient({ sharks, haloAt = [140, 130] }: { sharks: boolean; haloAt?: [number, number] }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const shade = useShade();
  const live = useLiveMotion();
  // Kept across pauses and resizes, so the sharks carry on where they were.
  const fish = useRef<Fish[]>(startingFish());
  const [hx, hy] = haloAt;

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    let sea = new Sea(cv, shade, fish.current, [hx, hy]);
    let stop: (() => void) | undefined;
    const run = () => {
      stop?.();
      sea.draw(sharks);
      if (live && sharks) {
        stop = frameLoop((dt, t) => {
          sea.step(dt, t);
          sea.draw(true);
        });
      }
    };
    const resize = new ResizeObserver(() => {
      sea = new Sea(cv, shade, fish.current, [hx, hy]);
      run();
    });
    resize.observe(cv);
    run();
    return () => {
      stop?.();
      resize.disconnect();
    };
  }, [shade, live, sharks, hx, hy]);

  return <canvas ref={ref} className="ambient" aria-hidden="true" />;
}
