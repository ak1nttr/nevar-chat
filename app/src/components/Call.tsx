import { useEffect, useState } from "react";
import { api, AppState, CallInfo, CallStats, describeError, Person } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { vkLabel } from "../keys";
import { Ambient } from "./Ambient";
import { Loader, Rolling } from "./Bits";
import { Icon } from "./Icon";
import { MusicPicker, MusicStrip, useSharers } from "./Music";
import { TopBar } from "./Mark";
import { displayLevel, TALKING, VoiceLane } from "./VoiceLane";

/** `?scene=music-pick` opens the music picker right away (used by the dev mock). */
const firstPopover = new URLSearchParams(location.search).get("scene") === "music-pick" ? "music" : null;

/** Everyone talks to everyone directly, so a call stays small. Matches what the mesh handles well. */
const MAX_PEOPLE = 10;

interface Props {
  state: AppState;
  call: CallInfo;
  toast: (text: string) => void;
  /** Folds the call into a strip so the rest of the app can be used. */
  onFold: () => void;
}

/** The whole window while a call is ringing (either way) or going on, with one or more people. */
export function CallScreen({ state, call, toast, onFold }: Props) {
  const active = call.phase === "active";
  const incoming = call.phase === "incoming";
  const stats = useCallStats(active);
  const elapsed = useElapsed(call.sinceMs);
  // One popover at a time over the tool island.
  const [popover, setPopover] = useState<"add" | "music" | null>(firstPopover);
  const adding = popover === "add";
  const setAdding = (on: boolean) => setPopover(on ? "add" : null);
  const sharers = useSharers(stats);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  // Esc closes the add-people picker, else folds the call away like going back.
  // Not while someone is calling us: that needs an answer.
  useEffect(() => {
    if (incoming) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (popover) setPopover(null);
      else onFold();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [incoming, popover, onFold]);

  if (incoming) return <Incoming call={call} run={run} />;

  // Whoever started the call can silence people or take them out.
  const iHost = call.host === state.me.id;

  // Live levels, 0..1 on the display scale.
  const levels: Record<string, number> = {};
  for (const m of call.members) {
    const peer = stats?.peers.find((p) => p.id === m.id);
    levels[m.id] = state.deafened ? 0 : displayLevel(peer?.level ?? 0);
  }
  const myLevel = active && stats?.transmitting && !state.muted ? displayLevel(stats.micLevel) : 0;
  const iTalk = myLevel > TALKING;

  const group = call.members.length + call.ringing.length > 1;
  const title = active ? joinNames(call.members.map((m) => m.name)) : joinNames(call.ringing.map((m) => m.name));

  const settings = state.settings;
  const [myNote, myTone]: [string, "on" | "warn" | undefined] = state.muted
    ? ["mikrofon kapalı", "warn"]
    : iTalk
      ? ["konuşuyorsun", "on"]
      : settings.mode === "pushToTalk"
        ? settings.push_to_talk_key == null
          ? ["bas-konuş tuşu yok", "warn"]
          : [`${vkLabel(settings.push_to_talk_key)} ile konuş`, undefined]
        : settings.mode === "open"
          ? ["mikrofon hep açık", undefined]
          : ["konuşunca açılır", undefined];

  // Online friends who aren't in the call yet.
  const inCall = new Set([...call.members, ...call.ringing].map((p) => p.id));
  const candidates = state.contacts.filter((c) => c.state === "friend" && c.online && !inCall.has(c.id));
  const full = inCall.size + 1 >= MAX_PEOPLE;

  return (
    <main className="screen live">
      <TopBar />

      <div className="live-body">
        <div className="live-top">
          {active ? (
            <button className="crumb" onClick={onFold} title="Görüşme sürerken uygulamayı kullan (Esc)">
              <Icon name="back" size={14} />
              Arkadaşlar
            </button>
          ) : (
            <span />
          )}
          {active && (
            <span className="live-meta">
              {stats && <Quality stats={stats} />}
              <Rolling className="num" text={formatDuration(elapsed)} />
            </span>
          )}
        </div>

        <div className="live-head">
          <p className="kicker">
            {active ? (
              <>
                <span className="dot live" />
                {group ? `görüşme · ${call.members.length + 1} kişi` : "görüşme"}
              </>
            ) : (
              <>
                <Loader />
                aranıyor
              </>
            )}
          </p>
          <h1 className={group ? "title several" : "title"}>{title}</h1>
          {state.deafened && <p className="their-status warn">Sesi kapattın, kimseyi duymuyorsun.</p>}
        </div>

        {active && <MusicStrip state={state} sharers={sharers} toast={toast} />}

        <div className="lanes">
          {call.members.map((m) => (
            <VoiceLane
              key={m.id}
              label={m.name}
              level={active ? levels[m.id] : 0}
              note={m.silenced ? "susturuldu" : relayNote(stats, m)}
              volume={m.volume}
              onVolume={active ? (v) => run(api.setVolume(m.id, v)) : undefined}
              host={call.host === m.id}
              silenced={m.silenced}
              onSilence={active && iHost ? (on) => run(api.silence(m.id, on)) : undefined}
              onRemove={active && iHost ? () => run(api.removeFromCall(m.id)) : undefined}
            />
          ))}
          {call.ringing.map((p) => (
            <VoiceLane key={p.id} label={p.name} ringing />
          ))}
          <VoiceLane label="Sen" level={myLevel} note={active ? myNote : undefined} noteTone={myTone} />
        </div>
      </div>

      <div className="island-wrap">
        {popover === "music" && <MusicPicker sharing={call.sharing} onClose={() => setPopover(null)} toast={toast} />}
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
          {active ? (
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
              <button
                className={call.sharing ? "tool sharing" : popover === "music" ? "tool on" : "tool"}
                onClick={() => setPopover(popover === "music" ? null : "music")}
                title={call.sharing ? `${call.sharing} paylaşılıyor` : "Müzik paylaş"}
                aria-label="Müzik paylaş"
                aria-expanded={popover === "music"}
              >
                <Icon name="music" />
              </button>
              <span className="island-sep" />
              <button className="hang" onClick={() => run(api.hangUp())}>
                {group ? "ayrıl" : "bitir"}
              </button>
            </>
          ) : (
            <button className="hang" onClick={() => run(api.hangUp())}>
              vazgeç
            </button>
          )}
        </div>
      </div>
    </main>
  );
}

/** Someone is calling: just who, and two buttons. Calm on purpose. */
function Incoming({ call, run }: { call: CallInfo; run: (p: Promise<unknown>) => void }) {
  return (
    <main className="screen live incoming">
      <Ambient sharks={false} haloAt={[150, 320]} />
      <TopBar />
      <div className="incoming-body">
        <p className="kicker accent">
          <Loader />
          gelen arama
        </p>
        <h1 className="title incoming-name">{call.caller?.name}</h1>
        <p className="incoming-sub">seni arıyor</p>
        {call.members.length > 0 && <p className="also">{joinNames(call.members.map((m) => m.name))} de görüşmede</p>}
      </div>
      <div className="incoming-actions">
        <button className="decline" onClick={() => run(api.declineCall())}>
          reddet
        </button>
        <button className="accept" onClick={() => run(api.acceptCall())} autoFocus>
          aç
        </button>
      </div>
    </main>
  );
}

/** "Emre", "Emre ve Selin", "Emre, Selin ve Mert", "Emre, Selin ve 2 kişi daha". */
export function joinNames(names: string[]): string {
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
  const grade = stats.route === "unknown" ? "bad" : (stats.pingMs ?? 0) < 60 ? "good" : (stats.pingMs ?? 0) < 150 ? "ok" : "bad";
  const route =
    stats.route === "direct" ? "Doğrudan bağlantı" : stats.route === "relayed" ? "Aracılı bağlantı" : "Bağlanıyor";
  return (
    <span
      className={`quality ${grade}`}
      title={`${route} · ses tamponu ${stats.bufferMs} ms · kayıp paket ${stats.lost} (kurtarılan ${stats.recovered})`}
    >
      <span className="dot" />
      <Rolling className="num" text={stats.pingMs == null ? "–" : String(stats.pingMs)} />
      <span className="unit">ms</span>
    </span>
  );
}

export function formatDuration(total: number): string {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
