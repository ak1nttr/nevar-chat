import { useState } from "react";
import { useAppState, useToasts } from "./hooks";
import { Home } from "./components/Home";
import { CallScreen } from "./components/Call";
import { SettingsPage } from "./components/SettingsPage";
import { UpdateBanner } from "./components/UpdateBanner";

export default function App() {
  const state = useAppState();
  const { toasts, push } = useToasts();
  // `?scene=settings` opens settings directly (used by the dev mock).
  const [settingsOpen, setSettingsOpen] = useState(() => location.search.includes("scene=settings"));

  if (!state) {
    return <div className="loading">nevar</div>;
  }

  // One screen at a time: a call (ringing either way, or live) takes the whole window.
  const call = state.call;

  return (
    <div className="app">
      <UpdateBanner inCall={call != null} />
      {call ? (
        <CallScreen state={state} call={call} toast={push} />
      ) : settingsOpen ? (
        <SettingsPage state={state} onClose={() => setSettingsOpen(false)} toast={push} />
      ) : (
        <Home state={state} onSettings={() => setSettingsOpen(true)} toast={push} />
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
