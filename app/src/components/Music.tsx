// Music sharing in a call: picking an app to share, and the strip that says what's playing.

import { useEffect, useRef, useState } from "react";
import { api, AppState, CallStats, describeError, MusicApp } from "../api";
import { Icon } from "./Icon";

/** Above this, someone's shared music counts as playing. */
const PLAYING = 0.002;
/** Quiet stretches in a song shorter than this don't count as stopping. */
const HOLD_MS = 2500;

/** Who in the call is sharing music, by ID. Holds through quiet passages. */
export function useSharers(stats: CallStats | null): string[] {
  const lastHeard = useRef<Record<string, number>>({});
  const now = Date.now();
  for (const p of stats?.peers ?? []) {
    if (p.musicLevel > PLAYING) lastHeard.current[p.id] = now;
  }
  return Object.entries(lastHeard.current)
    .filter(([, at]) => now - at < HOLD_MS)
    .map(([id]) => id);
}

/** Pick an app whose sound goes to everyone in the call. */
export function MusicPicker(props: {
  sharing: string | null;
  onClose: () => void;
  toast: (text: string) => void;
}) {
  const { onClose, toast } = props;
  const [apps, setApps] = useState<MusicApp[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    api
      .musicApps()
      .then(setApps)
      .catch((e) => {
        toast(describeError(e));
        setApps([]);
      });
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const share = async (app: MusicApp) => {
    setBusy(true);
    try {
      await api.shareApp(app.pid);
      onClose();
    } catch (e) {
      toast(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="picker" role="dialog" aria-label="Müzik paylaş">
      <div className="picker-head">
        <span>Müzik paylaş</span>
        <button className="icon-btn" onClick={onClose} aria-label="Kapat" title="Kapat">
          <Icon name="close" size={14} />
        </button>
      </div>
      {apps == null ? (
        <p className="picker-empty">Uygulamalara bakılıyor…</p>
      ) : apps.length === 0 ? (
        <p className="picker-empty">
          Şu an ses çıkaran bir uygulama yok. Spotify'da ya da tarayıcıda bir şarkı aç, sonra{" "}
          <button className="link" onClick={load}>
            tekrar bak
          </button>
          .
        </p>
      ) : (
        <ul>
          {apps.map((app) => (
            <li key={app.pid}>
              <button className="picker-row" disabled={busy} onClick={() => share(app)}>
                <span className={app.active ? "dot live" : "dot"} />
                <span className="picker-name">{app.name}</span>
                <span className="picker-action">
                  {props.sharing === app.name ? "paylaşılıyor" : app.active ? "çalıyor" : "sessiz"}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {props.sharing && (
        <button
          className="picker-stop"
          onClick={() => {
            api.stopSharing().catch((e) => toast(describeError(e)));
            onClose();
          }}
        >
          Paylaşmayı durdur
        </button>
      )}
      <p className="picker-note">Sadece o uygulamanın sesi gider; oyun ve bildirim sesleri gitmez.</p>
    </div>
  );
}

/** What music is on in the call: what we share, who else shares, and how loud it plays for us. */
export function MusicStrip(props: { state: AppState; sharers: string[]; toast: (text: string) => void }) {
  const { state, sharers, toast } = props;
  const call = state.call;
  if (!call) return null;
  const mine = call.sharing;
  const names = sharers.map((id) => call.members.find((m) => m.id === id)?.name.split(" ")[0] ?? "Biri");
  if (!mine && names.length === 0) return null;
  const volume = state.settings.music_volume;

  return (
    <div className="music-strip">
      <Icon name="music" size={15} />
      <span className="music-what">
        {mine && <span>{mine} paylaşıyorsun</span>}
        {names.length > 0 && <span>{names.join(", ")} müzik açtı</span>}
      </span>
      {names.length > 0 && (
        <input
          className="slider music-volume"
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={volume}
          style={{ "--fill": `${volume * 100}%` } as React.CSSProperties}
          onChange={(e) =>
            api
              .setSettings({ ...state.settings, music_volume: Number(e.target.value) })
              .catch((err) => toast(describeError(err)))
          }
          title={`Müzik sesi %${Math.round(volume * 100)}`}
          aria-label="Müzik sesi"
        />
      )}
      {mine && (
        <button className="btn ghost" onClick={() => api.stopSharing().catch((e) => toast(describeError(e)))}>
          Durdur
        </button>
      )}
    </div>
  );
}
