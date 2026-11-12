// The overlay window's page. Over a game it's a small see-through panel in a corner (who's
// talking, new messages); on the quick-menu shortcut the same window becomes the menu.

import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api, AppState, Contact, describeError, Notice } from "../api";
import { useAppState, useCallStats, useElapsed } from "../hooks";
import { hotkeyLabel } from "../keys";
import { formatDuration, joinNames } from "./Call";
import { ChatPage } from "./Chat";
import { Icon } from "./Icon";
import { Mark } from "./Mark";
import { displayLevel, TALKING } from "./VoiceLane";

/** How long a new message stays on the corner panel. */
const MESSAGE_MS = 10_000;
const MAX_MESSAGES = 3;

const scene = new URLSearchParams(location.search).get("scene");

export function OverlayApp() {
  const state = useAppState();
  const [menu, setMenu] = useState(scene === "menu");

  useEffect(() => {
    document.documentElement.classList.add("overlay-window");
    api.overlayMenuOpen().then(setMenu).catch(() => {});
    const unlisten = getCurrentWebviewWindow().listen<{ menu: boolean }>("overlay-mode", (e) => setMenu(e.payload.menu));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (!state) return null;
  return menu ? <QuickMenu state={state} /> : <Panel state={state} />;
}

// ---------- corner panel ----------

interface Toast {
  key: number;
  name: string;
  text: string;
}

/** New messages for a few seconds each, newest last. */
function useRecentMessages(): Toast[] {
  const [messages, setMessages] = useState<Toast[]>(
    scene === "overlay" ? [{ key: 0, name: "Selin", text: "geliyorum 2 dk" }] : [],
  );
  useEffect(() => {
    let next = 1;
    const unlisten = getCurrentWebviewWindow().listen<Notice>("notice", (e) => {
      const n = e.payload;
      if (n.kind !== "message") return;
      const key = next++;
      setMessages((m) => [...m, { key, name: n.name, text: n.text }].slice(-MAX_MESSAGES));
      setTimeout(() => setMessages((m) => m.filter((x) => x.key !== key)), MESSAGE_MS);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  return messages;
}

function Panel({ state }: { state: AppState }) {
  const o = state.settings.overlay;
  const call = state.call;
  const messages = useRecentMessages();
  const showCall = o.enabled && o.show_voice && call != null;
  const showMessages = o.enabled && o.show_messages && messages.length > 0;
  const has = showCall || showMessages;

  // The window is only shown while there's something on it.
  useEffect(() => {
    api.overlayHasContent(has).catch(() => {});
  }, [has]);

  return (
    <div className={`ov-panel ${o.corner}`}>
      {showCall && call.phase === "active" && <VoiceCard state={state} />}
      {showCall && call.phase === "incoming" && (
        <div className="ov-card">
          <p className="ov-line">
            <span className="ov-dot live" />
            <strong>{call.caller?.name}</strong>&nbsp;arıyor
          </p>
          <p className="ov-hint">{hotkeyLabel(state.settings.shortcuts.quick_menu)} ile aç</p>
        </div>
      )}
      {showCall && call.phase === "outgoing" && (
        <div className="ov-card">
          <p className="ov-line">
            <span className="ov-dot" />
            {joinNames(call.ringing.map((p) => p.name))} aranıyor…
          </p>
        </div>
      )}
      {showMessages &&
        messages.map((m) => (
          <div className="ov-card ov-message" key={m.key}>
            <strong>{m.name}</strong>
            <p>{m.text}</p>
          </div>
        ))}
    </div>
  );
}

/** Everyone in the call, lit while they talk. */
function VoiceCard({ state }: { state: AppState }) {
  const call = state.call!;
  const stats = useCallStats(true);
  const level = (id: string) => (state.deafened ? 0 : displayLevel(stats?.peers.find((p) => p.id === id)?.level ?? 0));
  const iTalk = !state.muted && !!stats?.transmitting && displayLevel(stats.micLevel) > TALKING;

  return (
    <div className="ov-card">
      {call.members.map((m) => (
        <p className="ov-line" key={m.id}>
          <span className={level(m.id) > TALKING ? "ov-dot live" : "ov-dot"} />
          <span className={level(m.id) > TALKING ? "ov-name talking" : "ov-name"}>{m.name}</span>
        </p>
      ))}
      <p className="ov-line">
        <span className={iTalk ? "ov-dot live" : "ov-dot"} />
        <span className={iTalk ? "ov-name talking" : "ov-name"}>Sen</span>
        {state.muted && <Icon name="micOff" size={13} />}
        {state.deafened && <Icon name="headphonesOff" size={13} />}
      </p>
    </div>
  );
}

// ---------- quick menu ----------

function QuickMenu({ state }: { state: AppState }) {
  const [chatWith, setChatWith] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = (p: Promise<unknown>) =>
    p.catch((e) => {
      setError(describeError(e));
      setTimeout(() => setError(null), 4000);
    });
  const close = () => run(api.closeQuickMenu());
  const settings = state.settings;

  // Esc closes the menu; inside a chat it goes back to the list first (the chat handles that).
  useEffect(() => {
    if (chatWith) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const byName = (a: Contact, b: Contact) => a.name.localeCompare(b.name, "tr");
  const friends = state.contacts.filter((c) => c.state === "friend");
  const online = friends.filter((c) => c.online).sort(byName);
  const offline = friends.filter((c) => !c.online).sort(byName);

  return (
    <div className="qm">
      <header className="qm-head">
        <span className="wordmark">
          <Mark />
          nevar
        </span>
        <span className="qm-head-actions">
          <button className="btn ghost" onClick={() => run(api.openMain())}>
            Uygulamayı aç
          </button>
          <button className="icon-btn" onClick={close} title="Kapat (Esc)" aria-label="Kapat">
            <Icon name="close" size={15} />
          </button>
        </span>
      </header>

      {chatWith ? (
        <div className="qm-chat">
          <ChatPage state={state} id={chatWith} onBack={() => setChatWith(null)} toast={setError} embedded />
        </div>
      ) : (
        <>
          {state.call && <CallCard state={state} run={run} />}
          <ul className="qm-friends">
            {[...online, ...offline].map((c) => (
              <li className={c.online ? "qm-friend" : "qm-friend offline"} key={c.id}>
                <span className={c.online ? "dot online" : "dot"} />
                <span className="qm-name">{c.name}</span>
                {c.online && c.inCall && <span className="row-note busy">görüşmede</span>}
                <button
                  className="row-btn square"
                  onClick={() => setChatWith(c.id)}
                  title={`${c.name} ile mesajlaş`}
                  aria-label={`${c.name} ile mesajlaş`}
                >
                  <Icon name="message" size={15} />
                  {c.unread > 0 && <span className="unread">{c.unread > 99 ? "99+" : c.unread}</span>}
                </button>
                {c.online && !c.inCall && (
                  <button
                    className="row-btn"
                    disabled={state.call != null && state.call.phase !== "active"}
                    onClick={() => run(api.startCall(c.id))}
                  >
                    <Icon name={state.call ? "userPlus" : "phone"} size={14} />
                    {state.call ? "Ekle" : "Ara"}
                  </button>
                )}
              </li>
            ))}
            {friends.length === 0 && <p className="qm-empty">Henüz arkadaşın yok.</p>}
          </ul>
          <footer className="qm-foot">
            <label className="qm-toggle">
              <input
                type="checkbox"
                role="switch"
                className="switch"
                checked={settings.overlay.enabled}
                onChange={(e) => run(api.setSettings({ ...settings, overlay: { ...settings.overlay, enabled: e.target.checked } }))}
              />
              Oyun içi pencere
            </label>
            <span className="qm-keys">{hotkeyLabel(settings.shortcuts.quick_menu)} · Esc</span>
          </footer>
        </>
      )}
      {error && <div className="qm-error">{error}</div>}
    </div>
  );
}

function CallCard({ state, run }: { state: AppState; run: (p: Promise<unknown>) => void }) {
  const call = state.call!;
  const elapsed = useElapsed(call.sinceMs);
  const active = call.phase === "active";
  const group = call.members.length + call.ringing.length > 1;

  if (call.phase === "incoming") {
    return (
      <section className="qm-call">
        <p className="qm-call-title">
          <span className="ov-dot live" />
          {call.caller?.name} arıyor
        </p>
        <div className="qm-call-tools">
          <button className="decline" onClick={() => run(api.declineCall())}>
            Reddet
          </button>
          <button className="accept" onClick={() => run(api.acceptCall())} autoFocus>
            <Icon name="phone" size={16} />
            Aç
          </button>
        </div>
      </section>
    );
  }

  const names = joinNames((active ? call.members : call.ringing).map((p) => p.name));
  return (
    <section className="qm-call">
      <p className="qm-call-title">
        <span className={active ? "ov-dot live" : "ov-dot"} />
        <span className="qm-call-names">{names}</span>
        <span className="qm-call-time">{active ? formatDuration(elapsed) : "Çalıyor…"}</span>
      </p>
      {call.sharing && (
        <p className="qm-sharing">
          <Icon name="music" size={14} />
          <span>{call.sharing} paylaşılıyor</span>
          <button className="link" onClick={() => run(api.stopSharing())}>
            Durdur
          </button>
        </p>
      )}
      <div className="qm-call-tools">
        {active && (
          <>
            <button
              className={state.muted ? "tool off" : "tool"}
              onClick={() => run(api.setMuted(!state.muted))}
              title={`${state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"} (${hotkeyLabel(state.settings.shortcuts.toggle_mute)})`}
              aria-label={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
            >
              <Icon name={state.muted ? "micOff" : "mic"} />
            </button>
            <button
              className={state.deafened ? "tool off" : "tool"}
              onClick={() => run(api.setDeafened(!state.deafened))}
              title={`${state.deafened ? "Sesi aç" : "Sesi kapat"} (${hotkeyLabel(state.settings.shortcuts.toggle_deafen)})`}
              aria-label={state.deafened ? "Sesi aç" : "Sesi kapat"}
            >
              <Icon name={state.deafened ? "headphonesOff" : "headphones"} />
            </button>
          </>
        )}
        <button className="hang" onClick={() => run(api.hangUp())}>
          <Icon name="hangUp" size={18} />
          {active ? (group ? "Ayrıl" : "Bitir") : "Vazgeç"}
        </button>
      </div>
    </section>
  );
}
