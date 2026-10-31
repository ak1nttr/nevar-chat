// Dev-only fake backend, so the UI can be designed in a plain browser.
// Loaded only by `pnpm dev` outside Tauri. Pick a scene with `?scene=home|call|incoming|ringing|settings|empty`.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AppState, CallStats, Devices } from "../api";

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
] as AppState["contacts"];

const state: AppState = {
  me,
  contacts: scene === "empty" ? [] : friends,
  call:
    scene === "call"
      ? { peer: friends[0].id, name: friends[0].name, phase: "active", sinceMs: Date.now() - 754_000 }
      : scene === "incoming"
        ? { peer: friends[1].id, name: friends[1].name, phase: "incoming", sinceMs: null }
        : scene === "ringing"
          ? { peer: friends[1].id, name: friends[1].name, phase: "outgoing", sinceMs: null }
          : null,
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

/** Steady levels for screenshots: the friend is talking, we're quiet. */
function stats(): CallStats {
  const t = Date.now() / 1000;
  const talk = (phase: number) => 0.06 + 0.03 * Math.sin(t * 9 + phase);
  return {
    route: "direct",
    pingMs: 6,
    bufferMs: 20,
    lost: 0,
    recovered: 0,
    micLevel: 0.004,
    speakerLevel: talk(0),
    transmitting: false,
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
