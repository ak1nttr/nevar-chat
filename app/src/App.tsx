import { useState } from "react";
import { useAppState, useToasts } from "./hooks";
import { Sidebar } from "./components/Sidebar";
import { CallStage, Home, IncomingCall } from "./components/Call";
import { SettingsPanel } from "./components/SettingsPanel";
import { UpdateBanner } from "./components/UpdateBanner";

export default function App() {
  const state = useAppState();
  const { toasts, push } = useToasts();
  const [settingsOpen, setSettingsOpen] = useState(false);

  if (!state) {
    return <div className="loading">nevar</div>;
  }

  const call = state.call;
  const inCall = call != null && call.phase !== "incoming";

  return (
    <div className={inCall ? "app in-call" : "app"}>
      <Sidebar state={state} onSettings={() => setSettingsOpen(true)} toast={push} />
      {inCall ? <CallStage state={state} call={call} toast={push} /> : <Home state={state} />}

      <UpdateBanner inCall={inCall} />
      {call?.phase === "incoming" && <IncomingCall call={call} toast={push} />}
      {settingsOpen && <SettingsPanel state={state} onClose={() => setSettingsOpen(false)} toast={push} />}

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
