import { useEffect, useState } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { api, AppState, CallInfo, CallStats, Contact, describeError } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { startRinging } from "../ringtone";
import { Avatar } from "./Avatar";
import { Icon, IconName } from "./Icon";
import { displayLevel, Meter } from "./Meter";

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

  const theirLevel = stats?.speakerLevel ?? 0;
  const myLevel = stats?.micLevel ?? 0;
  const iTransmit = stats?.transmitting ?? false;

  const theirStatus = !active
    ? "Çalıyor…"
    : state.deafened
      ? "Duymuyorsun"
      : displayLevel(theirLevel) > 0.15
        ? "Konuşuyor"
        : "Sessiz";
  const myStatus = state.muted ? "Mikrofon kapalı" : iTransmit ? "Konuşuyor" : "Sessiz";

  return (
    <main className="stage">
      <header className="stage-bar">
        <span className={active ? "chip live" : "chip ringing"}>
          <span className="dot" />
          {active ? "Canlı" : "Aranıyor"}
        </span>
        <span className="stage-title">{call.name}</span>
        <span className="stage-meta">
          {stats && <Quality stats={stats} />}
          {active && <span className="timer">{formatDuration(elapsed)}</span>}
        </span>
      </header>

      <div className="strips">
        <Strip
          id={call.peer}
          name={call.name}
          status={theirStatus}
          level={theirLevel}
          talking={active && theirStatus === "Konuşuyor"}
          off={state.deafened}
          ringing={!active}
        />
        <Strip
          id={state.me.id}
          name={state.me.name}
          you
          status={myStatus}
          level={iTransmit ? myLevel : 0}
          talking={active && myStatus === "Konuşuyor"}
          off={state.muted}
        />
      </div>

      <div className="dock">
        <DockButton
          icon={state.muted ? "micOff" : "mic"}
          label={state.muted ? "Kapalı" : "Mikrofon"}
          off={state.muted}
          onClick={() => run(api.setMuted(!state.muted))}
        />
        <DockButton
          icon={state.deafened ? "headphonesOff" : "headphones"}
          label={state.deafened ? "Kapalı" : "Ses"}
          off={state.deafened}
          onClick={() => run(api.setDeafened(!state.deafened))}
        />
        <DockButton icon="hangUp" label={active ? "Bitir" : "İptal"} danger onClick={() => run(api.hangUp())} />
      </div>

      {active && state.settings.mode === "pushToTalk" && state.settings.push_to_talk_key == null && (
        <p className="stage-hint">Bas-konuş açık ama tuş seçilmemiş. Ayarlar'dan bir tuş seç.</p>
      )}
    </main>
  );
}

interface StripProps {
  id: string;
  name: string;
  status: string;
  level: number;
  talking: boolean;
  off?: boolean;
  you?: boolean;
  ringing?: boolean;
}

function Strip({ id, name, status, level, talking, off, you, ringing }: StripProps) {
  return (
    <figure className={talking ? "strip talking" : "strip"}>
      <Avatar id={id} name={name} size={84} />
      <figcaption>
        <strong>{name}</strong>
        {you && <span className="you">sen</span>}
      </figcaption>
      <span className={off ? "strip-status off" : talking ? "strip-status live-text" : "strip-status"}>{status}</span>
      <Meter level={level} segments={18} off={off} className={ringing ? "sweep" : undefined} />
    </figure>
  );
}

function DockButton(props: { icon: IconName; label: string; off?: boolean; danger?: boolean; onClick: () => void }) {
  const cls = ["dock-button", props.off && "off", props.danger && "danger"].filter(Boolean).join(" ");
  return (
    <button className={cls} onClick={props.onClick}>
      <Icon name={props.icon} size={20} />
      <span>{props.label}</span>
    </button>
  );
}

function Quality({ stats }: { stats: CallStats }) {
  const ping = stats.pingMs ?? 0;
  const grade = stats.route === "unknown" ? "bad" : ping < 60 ? "good" : ping < 150 ? "ok" : "bad";
  const route = stats.route === "direct" ? "Doğrudan" : stats.route === "relayed" ? "Aracılı" : "Bağlanıyor";
  return (
    <span
      className={`quality ${grade}`}
      title={`Ses tamponu ${stats.bufferMs} ms · kayıp paket ${stats.lost} (kurtarılan ${stats.recovered})`}
    >
      <Bars grade={grade} />
      {route} · {stats.pingMs ?? "–"} ms
    </span>
  );
}

function Bars({ grade }: { grade: "good" | "ok" | "bad" }) {
  const lit = grade === "good" ? 3 : grade === "ok" ? 2 : 1;
  return (
    <span className="bars" aria-hidden="true">
      {[0, 1, 2].map((i) => (
        <span key={i} className={i < lit ? "on" : ""} />
      ))}
    </span>
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
      <div className="incoming" role="alertdialog" aria-label={`${call.name} arıyor`}>
        <span className="label accent">Gelen arama</span>
        <Avatar id={call.peer} name={call.name} size={72} />
        <h2>{call.name}</h2>
        <div className="incoming-actions">
          <button className="wide-button danger" onClick={() => run(api.declineCall())}>
            <Icon name="hangUp" size={18} />
            Reddet
          </button>
          <button className="wide-button live" onClick={() => run(api.acceptCall())} autoFocus>
            <Icon name="phone" size={18} />
            Aç
          </button>
        </div>
      </div>
    </div>
  );
}

/** The main area when no call is going on: who's around, and how to get started. */
export function Home({ state, toast }: { state: AppState; toast: (text: string) => void }) {
  const friends = state.contacts.filter((c) => c.state === "friend");
  const online = friends.filter((c) => c.online).sort((a, b) => a.name.localeCompare(b.name, "tr"));
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  return (
    <main className="home">
      <header className="stage-bar">
        <span className="label">Lobi</span>
        <span className="stage-meta">
          {online.length} / {friends.length} çevrimiçi
        </span>
      </header>

      {online.length > 0 ? (
        <div className="lobby">
          {online.map((c) => (
            <LobbyCard key={c.id} contact={c} onCall={() => run(api.startCall(c.id))} />
          ))}
        </div>
      ) : (
        <GettingStarted id={state.me.id} hasFriends={friends.length > 0} />
      )}
    </main>
  );
}

function LobbyCard({ contact: c, onCall }: { contact: Contact; onCall: () => void }) {
  return (
    <div className="lobby-card">
      <Avatar id={c.id} name={c.name} size={48} online />
      <strong>{c.name}</strong>
      <button className="wide-button live outline" onClick={onCall}>
        <Icon name="phone" size={16} />
        Ara
      </button>
    </div>
  );
}

function GettingStarted({ id, hasFriends }: { id: string; hasFriends: boolean }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(id);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="getting-started">
      <h1>{hasFriends ? "Şu an kimse çevrimiçi değil." : "Arkadaşlarını ekle, konuşmaya başla."}</h1>
      <p>
        {hasFriends
          ? "Arkadaşların uygulamayı açtığında burada görünecekler."
          : "nevar sunucu kullanmaz: arkadaşınla bilgisayarlarınız birbirine doğrudan bağlanır."}
      </p>

      <div className="code-card">
        <span className="label">Arkadaş kodun</span>
        <code>{id}</code>
        <button className="wide-button live outline" onClick={copy}>
          <Icon name={copied ? "check" : "copy"} size={16} />
          {copied ? "Kopyalandı" : "Kopyala"}
        </button>
      </div>

      <ol className="steps">
        <li>
          <span className="step-no">01</span>
          <span>
            <strong>Kodunu gönder</strong>
            Yukarıdaki kodu arkadaşına WhatsApp'tan at.
          </span>
        </li>
        <li>
          <span className="step-no">02</span>
          <span>
            <strong>Onun kodunu ekle</strong>
            Soldaki “Arkadaş ekle” kutusuna yapıştır.
          </span>
        </li>
        <li>
          <span className="step-no">03</span>
          <span>
            <strong>Ara</strong>
            İkiniz de çevrimiçiyken burada görünür, tek tıkla ararsın.
          </span>
        </li>
      </ol>
    </div>
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
