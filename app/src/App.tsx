import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { Notice, ShortcutUsed } from "./api";
import { useAppState, useToasts } from "./hooks";
import { notify, windowFocused } from "./notify";
import { listen } from "@tauri-apps/api/event";
import { messageChime, startRinging, toggleCue } from "./ringtone";
import { Home } from "./components/Home";
import { CallScreen } from "./components/Call";
import { CallBar } from "./components/CallBar";
import { ChatPage } from "./components/Chat";
import { SettingsPage } from "./components/SettingsPage";
import { UpdateBanner } from "./components/UpdateBanner";
import { ThemeContext, useApplyTheme } from "./theme";

type Page = { kind: "home" } | { kind: "settings" } | { kind: "chat"; id: string };

const scene = new URLSearchParams(location.search).get("scene");

/** `?scene=settings` and `?scene=chat` open those pages directly (used by the dev mock). */
function firstPage(): Page {
  if (scene === "settings") return { kind: "settings" };
  if (scene === "chat") return { kind: "chat", id: "cb72014711aa0e5c2b9d8f3e4a1b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b" };
  return { kind: "home" };
}

export default function App() {
  const state = useAppState();
  const shade = useApplyTheme(state?.settings.theme);
  const [page, setPage] = useState<Page>(firstPage);
  // Whether the call fills the window. Folded away, it's a strip under whatever page is open.
  const [callOpen, setCallOpen] = useState(scene !== "call-home");
  const foldCall = useCallback(() => setCallOpen(false), []);
  const pageRef = useRef(page);
  pageRef.current = page;

  const { toasts, push } = useToasts((n: Notice) => {
    if (n.kind !== "message") return true;
    const open = pageRef.current.kind === "chat" && pageRef.current.id === n.from;
    windowFocused().then((focused) => {
      if (!focused) {
        messageChime();
        notify(n.name, n.text);
        getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(() => {});
      } else if (!open) {
        messageChime();
      }
    });
    // An open chat shows the message itself.
    return !open;
  });

  // Someone ringing while nevar is hidden in the tray or behind a game: bring the window
  // back (without grabbing the keyboard) and say who it is.
  const ringing = state?.call?.phase === "incoming" ? state.call.caller?.name : undefined;
  useEffect(() => {
    if (!ringing) return;
    windowFocused().then((focused) => {
      if (focused) return;
      getCurrentWindow().show().catch(() => {});
      notify("Gelen arama", `${ringing} arıyor`);
    });
  }, [ringing]);

  // Shortcuts are mostly used inside a game, out of sight: a click says it worked.
  // Mute and deafen click "off" when they turn on.
  useEffect(() => {
    const unlisten = listen<ShortcutUsed>("shortcut", ({ payload: { shortcut, on } }) => {
      if (shortcut === "toggleMute" || shortcut === "toggleDeafen") toggleCue(on);
      else toggleCue(!on);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Ring while a call is ringing either way, even with the call folded away.
  const phase = state?.call?.phase;
  useEffect(() => {
    if (phase === "outgoing") return startRinging("outgoing");
    if (phase === "incoming") {
      const stop = startRinging("incoming");
      getCurrentWindow().requestUserAttention(UserAttentionType.Critical).catch(() => {});
      return stop;
    }
  }, [phase]);

  // Every new call starts full window.
  const hasCall = state?.call != null;
  const hadCall = useRef(hasCall);
  useEffect(() => {
    if (hadCall.current && !hasCall) setCallOpen(true);
    hadCall.current = hasCall;
  }, [hasCall]);

  if (!state) {
    return <div className="loading">nevar</div>;
  }

  // A call takes the whole window until folded away. Someone calling us always shows,
  // since it needs an answer.
  const call = state.call;
  const showCall = call != null && (callOpen || call.phase === "incoming");
  const home = () => setPage({ kind: "home" });

  return (
    <ThemeContext.Provider value={shade}>
    <div className="app">
      <UpdateBanner inCall={call != null} />
      {call && showCall ? (
        <CallScreen state={state} call={call} toast={push} onFold={foldCall} />
      ) : page.kind === "settings" ? (
        <SettingsPage state={state} onClose={home} toast={push} />
      ) : page.kind === "chat" ? (
        <ChatPage state={state} id={page.id} onBack={home} toast={push} />
      ) : (
        <Home
          state={state}
          onSettings={() => setPage({ kind: "settings" })}
          onChat={(id) => setPage({ kind: "chat", id })}
          toast={push}
        />
      )}
      {call && !showCall && <CallBar state={state} call={call} onOpen={() => setCallOpen(true)} toast={push} />}

      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div className="toast" key={t.id}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
    </ThemeContext.Provider>
  );
}
