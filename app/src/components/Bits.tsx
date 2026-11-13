// Small typographic pieces: numbers whose digits roll, and the dot-matrix waiting loader.

const DIGITS = Array.from({ length: 10 }, (_, d) => <span key={d}>{d}</span>);

/** Text whose digits roll to their new value (timers, ping, counts), set in Inter Tight with equal-width digits. */
export function Rolling({ text, className }: { text: string; className?: string }) {
  return (
    <span className={className ? `rolling ${className}` : "rolling"} role="img" aria-label={text}>
      {[...text].map((ch, i) =>
        ch >= "0" && ch <= "9" ? (
          <span className="rd" key={i} aria-hidden="true">
            <span className="rd-strip" style={{ transform: `translateY(${-Number(ch)}em)` }}>
              {DIGITS}
            </span>
          </span>
        ) : (
          <span key={i} aria-hidden="true">
            {ch}
          </span>
        ),
      )}
    </span>
  );
}

/** A 3×5 dot stack that strobes upward. Pure CSS; still under reduced motion. */
export function Loader() {
  return (
    <span className="loader" aria-hidden="true">
      {Array.from({ length: 15 }, (_, i) => (
        <i key={i} style={{ animationDelay: `${(4 - Math.floor(i / 3)) * 0.12 + (i % 3) * 0.04}s` }} />
      ))}
    </span>
  );
}
