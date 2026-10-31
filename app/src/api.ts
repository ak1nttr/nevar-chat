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
  /** In a call (or being rung into one) right now. */
  inCall: boolean;
  /** Messages from them not read yet. */
  unread: number;
}

export interface Person {
  id: string;
  name: string;
  /** How loud they play for us in calls: 1 as sent, 0 to 2. */
  volume: number;
}

export interface Message {
  /** Unique within the chat. */
  key: string;
  fromMe: boolean;
  text: string;
  atMs: number;
}

export interface MicTest {
  level: number;
  /** Whether this would go out in a call right now. */
  transmitting: boolean;
}

export interface CallInfo {
  phase: CallPhase;
  /** Who is ringing us (incoming calls only). */
  caller: Person | null;
  /** The people we're talking with. For an incoming call: who else is already in it. */
  members: Person[];
  /** People we rang into the call who haven't answered yet. */
  ringing: Person[];
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
  peers: PeerStats[];
}

export interface PeerStats {
  id: string;
  route: "direct" | "relayed" | "unknown";
  pingMs: number | null;
  bufferMs: number;
  lost: number;
  recovered: number;
  /** Their voice as last played. */
  level: number;
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
  | { kind: "message"; from: string; name: string; text: string }
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
  setVolume: (id: string, volume: number) => invoke<void>("set_volume", { id, volume }),
  sendMessage: (id: string, text: string) => invoke<void>("send_message", { id, text }),
  getMessages: (id: string) => invoke<Message[]>("get_messages", { id }),
  markRead: (id: string) => invoke<void>("mark_read", { id }),
  startMicTest: () => invoke<void>("start_mic_test"),
  stopMicTest: () => invoke<void>("stop_mic_test"),
  getMicTest: () => invoke<MicTest | null>("get_mic_test"),
};

/** Turns backend error text into something to show the user, in Turkish. */
export function describeError(e: unknown): string {
  const text = String(e);
  const known: [string, string][] = [
    ["doesn't look like a friend code", "Bu bir arkadaş kodu gibi görünmüyor."],
    ["your own code", "Bu senin kendi kodun."],
    ["already friends", "Zaten arkadaşsınız."],
    ["already in a call", "Zaten bir görüşmedesin."],
    ["already in the call", "Zaten görüşmede."],
    ["answer or decline the incoming call first", "Önce gelen aramayı aç ya da reddet."],
    ["only message friends", "Sadece arkadaşlarına mesaj atabilirsin."],
    ["message is empty", "Mesaj boş."],
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
    case "message":
      return `${n.name}: ${n.text}`;
    case "error":
      return describeError(n.message);
  }
}
