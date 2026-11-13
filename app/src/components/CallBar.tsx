import { api, AppState, CallInfo, describeError } from "../api";
import { useCallStats, useElapsed } from "../hooks";
import { Rolling } from "./Bits";
import { formatDuration, joinNames } from "./Call";
import { Icon } from "./Icon";
import { displayLevel, TALKING } from "./VoiceLane";
import { VoiceBand } from "./VoiceBand";

interface Props {
  state: AppState;
  call: CallInfo;
  /** Brings the call back to the whole window. */
  onOpen: () => void;
  toast: (text: string) => void;
}

/**
 * A folded-away call: a blue strip under the open page with who, how long, mute and hang up.
 * Its letters flow while someone in the call talks.
 */
export function CallBar({ state, call, onOpen, toast }: Props) {
  const active = call.phase === "active";
  const elapsed = useElapsed(call.sinceMs);
  const stats = useCallStats(active);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));
  const people = active ? call.members : call.ringing;
  const group = call.members.length + call.ringing.length > 1;

  const talking = active && !state.deafened
    ? call.members.filter((m) => displayLevel(stats?.peers.find((p) => p.id === m.id)?.level ?? 0) > TALKING)
    : [];
  const loudest = state.deafened ? 0 : Math.max(0, ...(stats?.peers.map((p) => displayLevel(p.level)) ?? []));
  const status = !active ? "çalıyor" : talking.length > 0 ? `${joinNames(talking.map((m) => m.name.split(" ")[0]))} konuşuyor` : "görüşme";

  return (
    <div className="callbar" role="region" aria-label="Görüşme">
      <VoiceBand
        letters={people.map((p) => p.name).join("")}
        level={loudest > TALKING ? loudest : 0}
        look="strip"
        className="callbar-field"
      />
      <button className="callbar-open" onClick={onOpen} title="Görüşmeye dön">
        <span className="callbar-text">
          <span className="callbar-status">{status}</span>
          <span className="callbar-names">{joinNames(people.map((p) => p.name))}</span>
        </span>
        {active && <Rolling className="num callbar-time" text={formatDuration(elapsed)} />}
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
