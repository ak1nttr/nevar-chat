import { useEffect } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { api, AppState, CallInfo, CallStats, describeError } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { vkLabel } from "../keys";
import { startRinging } from "../ringtone";
import { Icon } from "./Icon";
import { TopBar } from "./Mark";
import { BARS, displayLevel, TALKING, useHistory, useTicker, VoiceLane } from "./VoiceLane";

interface Props {
  state: AppState;
  call: CallInfo;
  toast: (text: string) => void;
}

/** The whole window while a call is ringing (either way) or going on. */
export function CallScreen({ state, call, toast }: Props) {
  const active = call.phase === "active";
  const incoming = call.phase === "incoming";
  const stats = useCallStats(active);
  const elapsed = useElapsed(call.sinceMs);
  const tick = useTicker(!active);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  useEffect(() => {
    if (call.phase === "outgoing") return startRinging("outgoing");
    if (call.phase === "incoming") {
      const stop = startRinging("incoming");
      getCurrentWindow().requestUserAttention(UserAttentionType.Critical).catch(() => {});
      return stop;
    }
  }, [call.phase]);

  const theirLevel = state.deafened ? 0 : displayLevel(stats?.speakerLevel ?? 0);
  const myLevel = stats?.transmitting && !state.muted ? displayLevel(stats.micLevel) : 0;
  const theirs = useHistory(theirLevel, active);
  const mine = useHistory(myLevel, active);
  const theyTalk = active && theirLevel > TALKING;
  const iTalk = active && myLevel > TALKING;

  const theirStatus = incoming
    ? "seni arıyor"
    : !active
      ? "Çalıyor…"
      : state.deafened
        ? "Sesi kapattın"
        : theyTalk
          ? "Konuşuyor"
          : "Sessiz";

  const settings = state.settings;
  const [myNote, myTone]: [string, "on" | "warn" | undefined] = state.muted
    ? ["Mikrofon kapalı", "warn"]
    : iTalk
      ? ["Konuşuyorsun", "on"]
      : settings.mode === "pushToTalk"
        ? settings.push_to_talk_key == null
          ? ["Bas-konuş tuşu seçilmemiş", "warn"]
          : [`${vkLabel(settings.push_to_talk_key)} tuşuyla konuş`, undefined]
        : settings.mode === "open"
          ? ["Mikrofon hep açık", undefined]
          : ["Konuşunca açılır", undefined];

  return (
    <main className="screen live">
      <TopBar />

      <div className="live-body">
        <div className="live-meta">
          {active && stats ? <Quality stats={stats} /> : <span />}
          {active && <span className="timer">{formatDuration(elapsed)}</span>}
        </div>

        <div className="live-head">
          <p className="kicker">{incoming ? "Gelen arama" : active ? "Görüşmede" : "Aranıyor"}</p>
          <h1 className="title">{call.name}</h1>
          <p className={theyTalk ? "their-status talking" : "their-status"}>{theirStatus}</p>
        </div>

        <div className="lanes">
          <VoiceLane label={call.name.split(" ")[0]} levels={active ? theirs : undefined} sweepAt={tick} />
          <VoiceLane
            label="Sen"
            note={incoming ? undefined : myNote}
            noteTone={myTone}
            levels={active ? mine : undefined}
            sweepAt={tick + BARS / 2}
          />
        </div>
      </div>

      <div className="island">
        {active && (
          <>
            <button
              className={state.muted ? "tool off" : "tool"}
              onClick={() => run(api.setMuted(!state.muted))}
              title={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
              aria-label={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
            >
              <Icon name={state.muted ? "micOff" : "mic"} />
            </button>
            <button
              className={state.deafened ? "tool off" : "tool"}
              onClick={() => run(api.setDeafened(!state.deafened))}
              title={state.deafened ? "Sesi aç" : "Sesi kapat"}
              aria-label={state.deafened ? "Sesi aç" : "Sesi kapat"}
            >
              <Icon name={state.deafened ? "headphonesOff" : "headphones"} />
            </button>
            <span className="island-sep" />
            <button className="hang" onClick={() => run(api.hangUp())}>
              <Icon name="hangUp" size={18} />
              Bitir
            </button>
          </>
        )}
        {call.phase === "outgoing" && (
          <button className="hang" onClick={() => run(api.hangUp())}>
            Vazgeç
          </button>
        )}
        {incoming && (
          <>
            <button className="decline" onClick={() => run(api.declineCall())}>
              Reddet
            </button>
            <button className="accept" onClick={() => run(api.acceptCall())} autoFocus>
              <Icon name="phone" size={18} />
              Aç
            </button>
          </>
        )}
      </div>
    </main>
  );
}

function Quality({ stats }: { stats: CallStats }) {
  const ping = stats.pingMs ?? 0;
  const grade = stats.route === "unknown" ? "bad" : ping < 60 ? "good" : ping < 150 ? "ok" : "bad";
  const route =
    stats.route === "direct" ? "Doğrudan bağlantı" : stats.route === "relayed" ? "Aracılı bağlantı" : "Bağlanıyor";
  return (
    <span
      className={`meta-item quality ${grade}`}
      title={`Ses tamponu ${stats.bufferMs} ms · kayıp paket ${stats.lost} (kurtarılan ${stats.recovered})`}
    >
      <span className="dot" />
      {route} · {stats.pingMs ?? "–"} ms
    </span>
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
