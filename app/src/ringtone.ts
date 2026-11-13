// Ring sounds synthesized with Web Audio, so no audio files ship with the app.

let ctx: AudioContext | null = null;

function audio(): AudioContext {
  ctx ??= new AudioContext();
  return ctx;
}

/** Two soft notes, like a doorbell. */
function chime(at: number, notes: number[], volume: number) {
  const ac = audio();
  notes.forEach((freq, i) => {
    const start = at + i * 0.18;
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0, start);
    gain.gain.linearRampToValueAtTime(volume, start + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.6);
    osc.connect(gain).connect(ac.destination);
    osc.start(start);
    osc.stop(start + 0.65);
  });
}

/** One short, quiet note for a new message. */
export function messageChime() {
  chime(audio().currentTime + 0.02, [988], 0.08);
}

/** Repeats a ring every couple of seconds until the returned function is called. */
export function startRinging(kind: "incoming" | "outgoing"): () => void {
  const notes = kind === "incoming" ? [784, 659] : [440, 440];
  const volume = kind === "incoming" ? 0.18 : 0.06;
  const ring = () => chime(audio().currentTime + 0.05, notes, volume);
  ring();
  const timer = setInterval(ring, kind === "incoming" ? 2000 : 3000);
  return () => clearInterval(timer);
}

/** A quick blip after a shortcut: rising when something turns back on, falling when it turns off. */
export function toggleCue(off: boolean) {
  const ac = audio();
  const notes = off ? [660, 440] : [440, 660];
  notes.forEach((freq, i) => {
    const start = ac.currentTime + 0.02 + i * 0.07;
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0, start);
    gain.gain.linearRampToValueAtTime(0.1, start + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.12);
    osc.connect(gain).connect(ac.destination);
    osc.start(start);
    osc.stop(start + 0.14);
  });
}
