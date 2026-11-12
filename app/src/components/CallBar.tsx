import { api, AppState, CallInfo, describeError } from "../api";
import { useElapsed } from "../hooks";
import { formatDuration, joinNames } from "./Call";
import { Icon } from "./Icon";

interface Props {
  state: AppState;
  call: CallInfo;
  /** Brings the call back to the whole window. */
  onOpen: () => void;
  toast: (text: string) => void;
}

/** A folded-away call: a strip under the open page with who, how long, mute and hang up. */
export function CallBar({ state, call, onOpen, toast }: Props) {
  const active = call.phase === "active";
  const elapsed = useElapsed(call.sinceMs);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));
  const people = active ? call.members : call.ringing;
  const group = call.members.length + call.ringing.length > 1;

  return (
    <div className="callbar" role="region" aria-label="Görüşme">
      <button className="callbar-open" onClick={onOpen} title="Görüşmeye dön">
        <span className={active ? "callbar-dot live" : "callbar-dot"} />
        <span className="callbar-text">
          <span className="callbar-names">{joinNames(people.map((p) => p.name))}</span>
          <span className="callbar-status">{active ? formatDuration(elapsed) : "Çalıyor…"}</span>
        </span>
      </button>
      {active && (
        <button
          className={state.muted ? "callbar-tool off" : "callbar-tool"}
          onClick={() => run(api.setMuted(!state.muted))}
          title={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
          aria-label={state.muted ? "Mikrofonu aç" : "Mikrofonu kapat"}
        >
          <Icon name={state.muted ? "micOff" : "mic"} size={16} />
        </button>
      )}
      <button
        className="callbar-tool hang"
        onClick={() => run(api.hangUp())}
        title={active ? (group ? "Ayrıl" : "Bitir") : "Vazgeç"}
        aria-label={active ? (group ? "Görüşmeden ayrıl" : "Görüşmeyi bitir") : "Aramaktan vazgeç"}
      >
        <Icon name="hangUp" size={16} />
      </button>
    </div>
  );
}
