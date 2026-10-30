// Typed bridge to the Rust side. Shapes mirror nevar-core's service types.

import { invoke } from "@tauri-apps/api/core";

export type ContactState = "outgoing" | "incoming" | "friend" | "declined";
export type CallPhase = "outgoing" | "incoming" | "active";
export type Mode = "voiceActivity" | "pushToTalk" | "open";

export interface Settings {
  mic: string | null;
  speakers: string | null;
  mode: Mode;
  push_to_talk_key: number | null;
  noise_suppression: boolean;
  echo_cancellation: boolean;
  auto_gain: boolean;
  output_volume: number;
}

export interface Contact {
  id: string;
  name: string;
  state: ContactState;
  online: boolean;
}

export interface CallInfo {
  peer: string;
  name: string;
  phase: CallPhase;
  sinceMs: number | null;
}

export interface AppState {
  me: { id: string; name: string; online: boolean };
  contacts: Contact[];
  call: CallInfo | null;
  settings: Settings;
  muted: boolean;
  deafened: boolean;
}

export interface CallStats {
  route: "direct" | "relayed" | "unknown";
  pingMs: number | null;
  bufferMs: number;
  lost: number;
  recovered: number;
  micLevel: number;
  speakerLevel: number;
  transmitting: boolean;
}

export interface Devices {
  inputs: string[];
  outputs: string[];
  defaultInput: string | null;
  defaultOutput: string | null;
}

export type Notice =
  | { kind: "friendRequest"; name: string }
  | { kind: "friendAccepted"; name: string }
  | { kind: "callDeclined"; name: string }
  | { kind: "noAnswer"; name: string }
  | { kind: "missedCall"; name: string }
  | { kind: "connectionLost"; name: string }
  | { kind: "error"; message: string };

export const api = {
  getState: () => invoke<AppState>("get_state"),
  getCallStats: () => invoke<CallStats | null>("get_call_stats"),
  listDevices: () => invoke<Devices>("list_devices"),
  setName: (name: string) => invoke<void>("set_name", { name }),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  setMuted: (muted: boolean) => invoke<void>("set_muted", { muted }),
  setDeafened: (deafened: boolean) => invoke<void>("set_deafened", { deafened }),
  addFriend: (code: string) => invoke<void>("add_friend", { code }),
  acceptRequest: (id: string) => invoke<void>("accept_request", { id }),
  declineRequest: (id: string) => invoke<void>("decline_request", { id }),
  removeContact: (id: string) => invoke<void>("remove_contact", { id }),
  startCall: (id: string) => invoke<void>("start_call", { id }),
  acceptCall: () => invoke<void>("accept_call"),
  declineCall: () => invoke<void>("decline_call"),
  hangUp: () => invoke<void>("hang_up"),
};

/** Turns backend error text into something to show the user, in Turkish. */
export function describeError(e: unknown): string {
  const text = String(e);
  const known: [string, string][] = [
    ["doesn't look like a friend code", "Bu bir arkadaş kodu gibi görünmüyor."],
    ["your own code", "Bu senin kendi kodun."],
    ["already friends", "Zaten arkadaşsınız."],
    ["already in a call", "Zaten bir görüşmedesin."],
    ["is offline", "Arkadaşın şu an çevrimdışı."],
    ["only call friends", "Sadece arkadaşlarını arayabilirsin."],
    ["nobody is calling", "Seni arayan kimse yok."],
    ["name can't be empty", "İsim boş olamaz."],
    ["connection lost", "Bağlantı koptu."],
    ["mic or speakers", "Mikrofon veya hoparlör açılamadı. Ayarlar'dan başka bir cihaz seç."],
  ];
  return known.find(([needle]) => text.includes(needle))?.[1] ?? text;
}

export function describeNotice(n: Notice): string {
  switch (n.kind) {
    case "friendRequest":
      return `${n.name} sana arkadaşlık isteği gönderdi.`;
    case "friendAccepted":
      return `${n.name} arkadaşlık isteğini kabul etti.`;
    case "callDeclined":
      return `${n.name} şu an konuşamıyor.`;
    case "noAnswer":
      return `${n.name} cevap vermedi.`;
    case "missedCall":
      return `Cevapsız arama: ${n.name}`;
    case "connectionLost":
      return `${n.name} ile bağlantı koptu.`;
    case "error":
      return describeError(n.message);
  }
}
