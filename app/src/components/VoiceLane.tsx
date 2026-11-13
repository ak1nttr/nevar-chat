// One row per person in a call: their name, a thin ASCII band that flows while they talk, and a
// short note (volume, relay, "çalıyor"). Quiet rows are still; only voice moves.

import { useState } from "react";
import { Loader } from "./Bits";
import { VoiceBand } from "./VoiceBand";

/** Above this (on the 0..1 display scale) someone counts as talking. */
export const TALKING = 0.15;

/** Maps RMS onto 0..1 on a log scale: -50 dBFS is empty, -10 dBFS is full. */
export function displayLevel(rms: number): number {
  const db = 20 * Math.log10(Math.max(rms, 1e-5));
  return Math.min(1, Math.max(0, (db + 50) / 40));
}

interface Props {
  label: string;
  /** Live voice, 0..1 on the display scale. */
  level?: number;
  note?: string;
  noteTone?: "on" | "warn";
  /** Rung into the call, not answered yet. */
  ringing?: boolean;
  /** Their volume (1 as sent, 0 to 2). With `onVolume`, the name opens a slider for it. */
  volume?: number;
  onVolume?: (volume: number) => void;
  /** They run the call. */
  host?: boolean;
  /** We host the call and turned their mic off. */
  silenced?: boolean;
  /** Only for the call's host: turn their mic off or back on, or take them out of the call. */
  onSilence?: (on: boolean) => void;
  onRemove?: () => void;
}

export function VoiceLane({
  label,
  level = 0,
  note,
  noteTone,
  ringing,
  volume = 1,
  onVolume,
  host,
  silenced,
  onSilence,
  onRemove,
}: Props) {
  const [adjusting, setAdjusting] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const toggle = (open: boolean) => {
    setAdjusting(open);
    setConfirmRemove(false);
  };
  const percent = Math.round(volume * 100);
  const talking = level > TALKING;
  // Below the talking line it's breath and room noise: keep the band still.
  const shown = talking ? level : 0;

  const meta = ringing ? (
    <span className="lane-note">
      <Loader /> çalıyor
    </span>
  ) : note ? (
    <span className={noteTone ? `lane-note ${noteTone}` : "lane-note"}>
      {note}
    </span>
  ) : onVolume ? (
    <span className={percent !== 100 ? "lane-note changed" : "lane-note"}>
      %{percent}
    </span>
  ) : null;

  return (
    <div
      className={talking ? "lane live" : ringing ? "lane waiting" : "lane"}
      // Right-clicking someone's lane opens their volume, like in other voice apps.
      onContextMenu={
        onVolume
          ? (e) => {
              e.preventDefault();
              toggle(true);
            }
          : undefined
      }
    >
      <div className="lane-row">
        <span className="lane-who">
          {onVolume ? (
            <button
              className={adjusting ? "lane-name open" : "lane-name"}
              onClick={() => toggle(!adjusting)}
              aria-expanded={adjusting}
              title={`${label} ses seviyesi (sağ tık da açar)`}
            >
              {label}
            </button>
          ) : (
            <span className="lane-name">{label}</span>
          )}
          {host && (
            <span className="lane-host" title="Görüşmeyi başlatan">
              kurucu
            </span>
          )}
        </span>
        <VoiceBand letters={label} level={shown} />
        {meta}
      </div>
      {adjusting && onVolume && (
        <div className="lane-slider">
          <input
            className="slider"
            type="range"
            min={0}
            max={2}
            step={0.05}
            value={volume}
            style={
              { "--fill": `${(volume / 2) * 100}%` } as React.CSSProperties
            }
            onChange={(e) => onVolume(Number(e.target.value))}
            aria-label={`${label} ses seviyesi`}
          />
          <span className="lane-slider-value">%{percent}</span>
          {percent !== 100 && (
            <button className="btn ghost" onClick={() => onVolume(1)}>
              Sıfırla
            </button>
          )}
        </div>
      )}
      {adjusting && (onSilence || onRemove) && (
        <div className="lane-host-actions">
          {confirmRemove ? (
            <>
              <span className="lane-confirm">
                {label} görüşmeden çıkarılsın mı?
              </span>
              <button className="btn danger" onClick={onRemove}>
                Çıkar
              </button>
              <button
                className="btn ghost"
                onClick={() => setConfirmRemove(false)}
              >
                Vazgeç
              </button>
            </>
          ) : (
            <>
              {onSilence && (
                <button
                  className="btn ghost"
                  onClick={() => onSilence(!silenced)}
                  title={silenced ? "Kapattığın mikrofonunu geri açar" : "Mikrofonunu kapatır, kendisi tekrar açabilir"}
                >
                  {silenced ? "Sesini aç" : "Herkes için sustur"}
                </button>
              )}
              {onRemove && (
                <button
                  className="btn ghost danger-text"
                  onClick={() => setConfirmRemove(true)}
                >
                  Görüşmeden çıkar
                </button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
