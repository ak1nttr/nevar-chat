import { useEffect, useRef, useState } from "react";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { Notice } from "./api";
import { useAppState, useToasts } from "./hooks";
import { notify, windowFocused } from "./notify";
import { messageChime } from "./ringtone";
import { Home } from "./components/Home";
import { CallScreen } from "./components/Call";
import { ChatPage } from "./components/Chat";
import { SettingsPage } from "./components/SettingsPage";
import { UpdateBanner } from "./components/UpdateBanner";

type Page = { kind: "home" } | { kind: "settings" } | { kind: "chat"; id: string };

/** `?scene=settings` and `?scene=chat` open those pages directly (used by the dev mock). */
function firstPage(): Page {
  const scene = new URLSearchParams(location.search).get("scene");
  if (scene === "settings") return { kind: "settings" };
  if (scene === "chat") return { kind: "chat", id: "cb72014711aa0e5c2b9d8f3e4a1b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b" };
  return { kind: "home" };
}

export default function App() {
  const state = useAppState();
  const [page, setPage] = useState<Page>(firstPage);
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

  if (!state) {
    return <div className="loading">nevar</div>;
  }

  // One screen at a time: a call (ringing either way, or live) takes the whole window.
  const call = state.call;
  const home = () => setPage({ kind: "home" });

  return (
    <div className="app">
      <UpdateBanner inCall={call != null} />
      {call ? (
        <CallScreen state={state} call={call} toast={push} />
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

      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div className="toast" key={t.id}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
