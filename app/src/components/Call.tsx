import { useEffect } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { api, AppState, CallInfo, CallStats, describeError } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { startRinging } from "../ringtone";
import { Avatar, displayLevel } from "./Avatar";
import { Icon } from "./Icon";

interface Props {
  state: AppState;
  call: CallInfo;
  toast: (text: string) => void;
}

/** The main area while ringing someone or talking. */
export function CallStage({ state, call, toast }: Props) {
  const active = call.phase === "active";
  const stats = useCallStats(active);
  const elapsed = useElapsed(call.sinceMs);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  useEffect(() => {
    if (call.phase === "outgoing") return startRinging("outgoing");
  }, [call.phase]);

  const myLevel = stats && stats.transmitting && !state.muted ? displayLevel(stats.micLevel) : 0;
  const theirLevel = stats && !state.deafened ? displayLevel(stats.speakerLevel) : 0;

  return (
    <main className="stage">
      <div className="stage-people">
        <Person id={call.peer} name={call.name} level={theirLevel} />
        <Person id={state.me.id} name={state.me.name} level={myLevel} you />
      </div>

      <p className="stage-status">
        {call.phase === "outgoing" ? <span className="pulse">Aranıyor…</span> : formatDuration(elapsed)}
      </p>
      {stats && <Quality stats={stats} />}

      <div className="controls">
        <button
          className={state.muted ? "control off" : "control"}
          onClick={() => run(api.setMuted(!state.muted))}
          title={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
        >
          <Icon name={state.muted ? "micOff" : "mic"} size={22} />
        </button>
        <button
          className={state.deafened ? "control off" : "control"}
          onClick={() => run(api.setDeafened(!state.deafened))}
          title={state.deafened ? "Sesi aç" : "Hiçbir şey duyma"}
        >
          <Icon name={state.deafened ? "headphonesOff" : "headphones"} size={22} />
        </button>
        <button className="control hang-up" onClick={() => run(api.hangUp())} title={active ? "Aramayı bitir" : "İptal"}>
          <Icon name="hangUp" size={24} />
        </button>
      </div>
      {active && state.settings.mode === "pushToTalk" && state.settings.push_to_talk_key == null && (
        <p className="hint center">Bas-konuş açık ama tuş seçilmemiş. Ayarlar'dan bir tuş seç.</p>
      )}
    </main>
  );
}

function Person({ id, name, level, you }: { id: string; name: string; level: number; you?: boolean }) {
  return (
    <figure className="person">
      <Avatar id={id} name={name} size={112} level={level} />
      <figcaption>{you ? `${name} (sen)` : name}</figcaption>
    </figure>
  );
}

function Quality({ stats }: { stats: CallStats }) {
  const route = stats.route === "direct" ? "Doğrudan bağlantı" : stats.route === "relayed" ? "Aracı sunucu üzerinden" : "Bağlanıyor";
  const ping = stats.pingMs ?? 0;
  const grade = ping < 60 ? "good" : ping < 150 ? "ok" : "bad";
  return (
    <p className={`quality ${grade}`} title={`Ses tamponu ${stats.bufferMs} ms · kayıp paket ${stats.lost} (kurtarılan ${stats.recovered})`}>
      <span className="dot" />
      {route} · {stats.pingMs ?? "?"} ms
    </p>
  );
}

/** Full-window overlay while a friend is calling us. */
export function IncomingCall({ call, toast }: { call: CallInfo; toast: (text: string) => void }) {
  useEffect(() => {
    const stop = startRinging("incoming");
    getCurrentWindow().requestUserAttention(UserAttentionType.Critical).catch(() => {});
    return stop;
  }, []);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  return (
    <div className="overlay">
      <div className="incoming">
        <Avatar id={call.peer} name={call.name} size={96} />
        <h2>{call.name}</h2>
        <p className="pulse">seni arıyor</p>
        <div className="controls">
          <button className="control hang-up" onClick={() => run(api.declineCall())} title="Reddet">
            <Icon name="hangUp" size={24} />
          </button>
          <button className="control answer" onClick={() => run(api.acceptCall())} title="Aç" autoFocus>
            <Icon name="phone" size={24} />
          </button>
        </div>
      </div>
    </div>
  );
}

/** The main area when nothing is going on. */
export function Home({ state }: { state: AppState }) {
  const online = state.contacts.filter((c) => c.state === "friend" && c.online);
  return (
    <main className="home">
      <h1>nevar?</h1>
      {online.length > 0 ? (
        <p>
          {online.map((c) => c.name).join(", ")} çevrimiçi. Aramak için soldaki telefon simgesine bas.
        </p>
      ) : (
        <p>Şu an kimse çevrimiçi değil. Arkadaşların uygulamayı açınca burada göreceksin.</p>
      )}
      <ol className="steps">
        <li>
          <strong>Kodunu paylaş.</strong> Soldaki “Arkadaş kodum”a basınca kopyalanır.
        </li>
        <li>
          <strong>Arkadaşının kodunu ekle.</strong> En alttaki kutuya yapıştır.
        </li>
        <li>
          <strong>Ara.</strong> İkiniz de çevrimiçiyken telefon simgesine bas.
        </li>
      </ol>
    </main>
  );
}

function formatDuration(total: number): string {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
