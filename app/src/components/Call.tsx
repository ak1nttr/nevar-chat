import { useEffect, useState } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { api, AppState, CallInfo, CallStats, describeError, Person } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { vkLabel } from "../keys";
import { startRinging } from "../ringtone";
import { Icon } from "./Icon";
import { TopBar } from "./Mark";
import { BARS, displayLevel, TALKING, useHistories, useHistory, useTicker, VoiceLane } from "./VoiceLane";

/** Everyone talks to everyone directly, so a call stays small. Matches what the mesh handles well. */
const MAX_PEOPLE = 10;

interface Props {
  state: AppState;
  call: CallInfo;
  toast: (text: string) => void;
}

/** The whole window while a call is ringing (either way) or going on, with one or more people. */
export function CallScreen({ state, call, toast }: Props) {
  const active = call.phase === "active";
  const incoming = call.phase === "incoming";
  const stats = useCallStats(active);
  const elapsed = useElapsed(call.sinceMs);
  const tick = useTicker(!active);
  const [adding, setAdding] = useState(false);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  useEffect(() => {
    if (call.phase === "outgoing") return startRinging("outgoing");
    if (call.phase === "incoming") {
      const stop = startRinging("incoming");
      getCurrentWindow().requestUserAttention(UserAttentionType.Critical).catch(() => {});
      return stop;
    }
  }, [call.phase]);

  // Live levels, 0..1 on the display scale.
  const levels: Record<string, number> = {};
  for (const m of call.members) {
    const peer = stats?.peers.find((p) => p.id === m.id);
    levels[m.id] = state.deafened ? 0 : displayLevel(peer?.level ?? 0);
  }
  const myLevel = stats?.transmitting && !state.muted ? displayLevel(stats.micLevel) : 0;
  const histories = useHistories(levels, active);
  const mine = useHistory(myLevel, active);
  const talking = active ? call.members.filter((m) => levels[m.id] > TALKING) : [];
  const iTalk = active && myLevel > TALKING;

  const group = call.members.length + call.ringing.length > 1;
  const compact = call.members.length + call.ringing.length > 2;

  const title = incoming
    ? call.caller?.name ?? ""
    : active
      ? joinNames(call.members.map((m) => m.name))
      : joinNames(call.ringing.map((m) => m.name));
  const status = incoming
    ? "seni arıyor"
    : !active
      ? "Çalıyor…"
      : state.deafened
        ? "Sesi kapattın"
        : talking.length > 0
          ? `${joinNames(talking.map((m) => m.name.split(" ")[0]))} konuşuyor`
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

  // Online friends who aren't in the call yet.
  const inCall = new Set([...call.members, ...call.ringing].map((p) => p.id));
  const candidates = state.contacts.filter((c) => c.state === "friend" && c.online && !inCall.has(c.id));
  const full = inCall.size + 1 >= MAX_PEOPLE;

  return (
    <main className="screen live">
      <TopBar />

      <div className="live-body">
        <div className="live-meta">
          {active && stats ? <Quality stats={stats} /> : <span />}
          {active && <span className="timer">{formatDuration(elapsed)}</span>}
        </div>

        <div className="live-head">
          <p className="kicker">
            {incoming ? "Gelen arama" : active ? (group ? `Görüşmede · ${call.members.length + 1} kişi` : "Görüşmede") : "Aranıyor"}
          </p>
          <h1 className={group ? "title several" : "title"}>{title}</h1>
          <p className={talking.length > 0 ? "their-status talking" : "their-status"}>{status}</p>
          {incoming && call.members.length > 0 && (
            <p className="also">{joinNames(call.members.map((m) => m.name))} de görüşmede</p>
          )}
        </div>

        <div className={compact ? "lanes compact" : "lanes"}>
          {incoming && call.caller ? (
            <VoiceLane label={call.caller.name} sweepAt={tick} />
          ) : (
            <>
              {call.members.map((m) => (
                <VoiceLane
                  key={m.id}
                  label={m.name}
                  note={relayNote(stats, m)}
                  levels={active ? histories[m.id] : undefined}
                  sweepAt={tick}
                  compact={compact}
                />
              ))}
              {call.ringing.map((p, i) => (
                <VoiceLane key={p.id} label={p.name} note="Çalıyor…" sweepAt={tick + i * 7} compact={compact} />
              ))}
            </>
          )}
          <VoiceLane
            label="Sen"
            note={incoming ? undefined : myNote}
            noteTone={myTone}
            levels={active ? mine : undefined}
            sweepAt={tick + BARS / 2}
            compact={compact}
          />
        </div>
      </div>

      <div className="island-wrap">
        {adding && (
          <div className="picker" role="dialog" aria-label="Görüşmeye ekle">
            <div className="picker-head">
              <span>Görüşmeye ekle</span>
              <button className="icon-btn" onClick={() => setAdding(false)} aria-label="Kapat" title="Kapat">
                <Icon name="close" size={14} />
              </button>
            </div>
            {candidates.length === 0 ? (
              <p className="picker-empty">Şu an çevrimiçi başka arkadaşın yok.</p>
            ) : (
              <ul>
                {candidates.map((c) => (
                  <li key={c.id}>
                    <button
                      className="picker-row"
                      onClick={() => {
                        setAdding(false);
                        run(api.startCall(c.id));
                      }}
                    >
                      <span className="dot online" />
                      <span className="picker-name">{c.name}</span>
                      <span className="picker-action">Ara</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

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
              <button
                className={adding ? "tool on" : "tool"}
                onClick={() => setAdding(!adding)}
                disabled={full}
                title={full ? `Bir görüşmede en fazla ${MAX_PEOPLE} kişi olabilir` : "Görüşmeye ekle"}
                aria-label="Görüşmeye ekle"
                aria-expanded={adding}
              >
                <Icon name="userPlus" />
              </button>
              <span className="island-sep" />
              <button className="hang" onClick={() => run(api.hangUp())}>
                <Icon name="hangUp" size={18} />
                {group ? "Ayrıl" : "Bitir"}
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
      </div>
    </main>
  );
}

/** "Emre", "Emre ve Selin", "Emre, Selin ve Mert", "Emre, Selin ve 2 kişi daha". */
function joinNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  if (names.length <= 3) return `${names.slice(0, -1).join(", ")} ve ${names[names.length - 1]}`;
  return `${names.slice(0, 2).join(", ")} ve ${names.length - 2} kişi daha`;
}

/** Only worth mentioning when someone's voice takes the long way, through a relay. */
function relayNote(stats: CallStats | null, person: Person): string | undefined {
  const peer = stats?.peers.find((p) => p.id === person.id);
  return peer?.route === "relayed" ? `aracılı · ${peer.pingMs ?? "–"} ms` : undefined;
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
