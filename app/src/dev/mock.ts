// Dev-only fake backend, so the UI can be designed in a plain browser.
// Loaded only by `pnpm dev` outside Tauri. Pick a scene with
// `?scene=home|chat|call|group|incoming|incoming-group|ringing|settings|empty|update`.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AppState, CallStats, Devices, Message } from "../api";

const scene = new URLSearchParams(location.search).get("scene") ?? "home";

// Headless Edge won't open a window narrower than about 500px, so scripts/shot.sh passes the app's real
// window size as `?frame=380x660` and the app is drawn in a box of that size.
const frame = new URLSearchParams(location.search).get("frame")?.match(/^(\d+)x(\d+)$/);
if (frame) {
  const style = document.createElement("style");
  style.textContent = `html,body{background:#888}#root{width:${frame[1]}px;height:${frame[2]}px;overflow:hidden;background:#fff}.app,.loading{height:100%!important}`;
  document.head.append(style);
}

const me = { id: "21cb7ffb12beccbafa316c0ac7e1289379009d52626a86df8e231316b1d06598", name: "Akın", online: true };
const friends = [
  { id: "cb72014711aa0e5c2b9d8f3e4a1b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b", name: "Emre", state: "friend", online: true },
  { id: "9a3f55e01b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e", name: "Selin Yıldız", state: "friend", online: true },
  { id: "4be81c0d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b", name: "Mert", state: "friend", online: false },
  { id: "e0c4d7a12b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e", name: "Deniz Kaya", state: "friend", online: false },
  { id: "77d1a9b03c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f", name: "İrem", state: "incoming", online: false },
  { id: "5f0e2c8a4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a", name: "Burak", state: "outgoing", online: false },
].map((f) => ({ inCall: false, unread: 0, ...f })) as AppState["contacts"];
// Selin is talking with someone else; Emre wrote while we were away.
friends[1].inCall = scene === "home";
friends[0].unread = scene === "home" ? 2 : 0;

const person = (f: { id: string; name: string }) => ({ id: f.id, name: f.name, volume: 1 });
// Someone in the group call who isn't one of our friends, turned down a little.
const guest = { id: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90", name: "Kaan", volume: 0.7 };

const minutesAgo = (m: number) => Date.now() - m * 60_000;
const messages: Message[] = [
  { key: "t1", fromMe: false, text: "akşam maç var, girecek misin?", atMs: minutesAgo(26 * 60) },
  { key: "m1", fromMe: true, text: "girerim, 9 gibi", atMs: minutesAgo(26 * 60 - 3) },
  { key: "t2", fromMe: false, text: "selam\nbugün de girelim mi?", atMs: minutesAgo(14) },
  { key: "t3", fromMe: false, text: "Selin de gelecekmiş", atMs: minutesAgo(13) },
  { key: "m2", fromMe: true, text: "olur, yarım saate bilgisayardayım", atMs: minutesAgo(2) },
];
let micTestOn = false;

function callFor(scene: string): AppState["call"] {
  const call = { caller: null, members: [], ringing: [], sinceMs: null };
  switch (scene) {
    case "call":
      return { ...call, phase: "active", members: [person(friends[0])], sinceMs: Date.now() - 754_000 };
    case "group":
      return {
        ...call,
        phase: "active",
        members: [person(friends[0]), person(friends[1]), guest],
        ringing: [person(friends[2])],
        sinceMs: Date.now() - 1_954_000,
      };
    case "incoming":
      return { ...call, phase: "incoming", caller: person(friends[1]) };
    case "incoming-group":
      return { ...call, phase: "incoming", caller: person(friends[1]), members: [person(friends[0]), guest] };
    case "ringing":
      return { ...call, phase: "outgoing", ringing: [person(friends[1])] };
    default:
      return null;
  }
}

const state: AppState = {
  me,
  contacts: scene === "empty" ? [] : friends,
  call: callFor(scene),
  settings: {
    mic: null,
    speakers: null,
    mode: "voiceActivity",
    push_to_talk_key: null,
    noise_suppression: true,
    echo_cancellation: true,
    auto_gain: true,
    output_volume: 1,
  },
  muted: false,
  deafened: false,
};

const devices: Devices = {
  inputs: ["Mikrofon (C-Media(R) Audio)", "Kulaklık (EarPods)"],
  outputs: ["LS24AG32x (NVIDIA High Definition Audio)", "Kulaklık (EarPods)", "Hoparlör (C-Media(R) Audio)"],
  defaultInput: "Kulaklık (EarPods)",
  defaultOutput: "Kulaklık (EarPods)",
};

/** Steady levels for screenshots: the first person is talking, the others and we are quiet. */
function stats(): CallStats {
  const t = Date.now() / 1000;
  const talk = (phase: number) => 0.06 + 0.03 * Math.sin(t * 9 + phase);
  const peers = (state.call?.members ?? []).map((m, i) => ({
    id: m.id,
    // The one who isn't our friend reaches us through a relay.
    route: (m.id === guest.id ? "relayed" : "direct") as "relayed" | "direct",
    pingMs: m.id === guest.id ? 48 : 6,
    bufferMs: 20,
    lost: 0,
    recovered: 0,
    level: i === 0 ? talk(i) : 0.002,
  }));
  return {
    route: peers.some((p) => p.route === "relayed") ? "relayed" : "direct",
    pingMs: Math.max(6, ...peers.map((p) => p.pingMs)),
    bufferMs: 20,
    lost: 0,
    recovered: 0,
    micLevel: 0.004,
    speakerLevel: talk(0),
    transmitting: false,
    peers,
  };
}

mockWindows("main");
mockIPC(
  (cmd, args) => {
    switch (cmd) {
      case "get_state":
        return state;
      case "get_call_stats":
        return state.call?.phase === "active" ? stats() : null;
      case "list_devices":
        return devices;
      case "get_messages":
        return messages;
      case "send_message":
        messages.push({ key: `m${messages.length}`, fromMe: true, text: (args as { text: string }).text, atMs: Date.now() });
        return;
      case "start_mic_test":
        micTestOn = true;
        return;
      case "stop_mic_test":
        micTestOn = false;
        return;
      case "get_mic_test":
        return micTestOn ? { level: 0.05 + 0.03 * Math.sin(Date.now() / 120), transmitting: true } : null;
      case "plugin:app|version":
        return "0.2.1";
      case "plugin:updater|check":
        return scene === "update" ? { rid: 1, currentVersion: "0.2.1", version: "0.3.0", date: null, body: "", rawJson: {} } : null;
      case "set_muted":
        state.muted = (args as { muted: boolean }).muted;
        return;
      case "set_deafened":
        state.deafened = (args as { deafened: boolean }).deafened;
        return;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);
