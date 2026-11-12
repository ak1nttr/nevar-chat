import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, AppState, describeError, Message } from "../api";
import { Icon } from "./Icon";
import { TopBar } from "./Mark";

/** Must match the core's limit. */
const MAX_CHARS = 2000;
/** Messages closer together than this, from the same person, share one name line. */
const GROUP_MS = 5 * 60 * 1000;

interface Props {
  state: AppState;
  id: string;
  onBack: () => void;
  toast: (text: string) => void;
  /** Inside the quick menu: no window bar. */
  embedded?: boolean;
}

/** Messages with one friend. They only go through while both of you are online. */
export function ChatPage({ state, id, onBack, toast, embedded }: Props) {
  const friend = state.contacts.find((c) => c.id === id && c.state === "friend");
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  // The state is refetched on every backend change, including a new message.
  useEffect(() => {
    api.getMessages(id).then(setMessages).catch(() => {});
  }, [state, id]);

  const unread = friend?.unread ?? 0;
  useEffect(() => {
    if (unread > 0) api.markRead(id).catch(() => {});
  }, [unread, id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onBack();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack]);

  // Keep the newest message in view.
  useLayoutEffect(() => {
    const el = list.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length]);

  // They removed us, or we removed them from somewhere else.
  useEffect(() => {
    if (!friend) onBack();
  }, [friend, onBack]);
  if (!friend) return null;

  const send = async () => {
    const text = draft.trim();
    if (!text || sending) return;
    setSending(true);
    try {
      await api.sendMessage(id, text);
      setDraft("");
    } catch (e) {
      toast(describeError(e));
    } finally {
      setSending(false);
    }
  };

  const status = friend.inCall ? "Görüşmede" : friend.online ? "Çevrimiçi" : "Çevrimdışı";

  return (
    <main className="screen chat">
      {!embedded && <TopBar>
        {friend.online && !friend.inCall && (
          <button
            className="icon-btn"
            onClick={() => run(api.startCall(id))}
            title={`${friend.name} kişisini ara`}
            aria-label={`${friend.name} kişisini ara`}
          >
            <Icon name="phone" size={16} />
          </button>
        )}
      </TopBar>}

      <div className="chat-head">
        <button className="crumb" onClick={onBack} title="Geri (Esc)">
          <Icon name="back" size={14} />
          Arkadaşlar
        </button>
        <h1 className="title">{friend.name}</h1>
        <p className="status-line">
          <span className={friend.online ? "dot online" : "dot"} />
          {status}
        </p>
      </div>

      <div className="messages" ref={list}>
        {messages.length === 0 ? (
          <p className="messages-empty">
            Henüz mesaj yok. Mesajlar ikiniz de çevrimiçiyken gider ve yalnızca bu bilgisayarda saklanır.
          </p>
        ) : (
          messages.map((m, i) => {
            const prev = messages[i - 1];
            const newDay = !prev || dayKey(prev.atMs) !== dayKey(m.atMs);
            const showName = newDay || prev.fromMe !== m.fromMe || m.atMs - prev.atMs > GROUP_MS;
            return (
              <div key={m.key}>
                {newDay && <p className="day">{dayLabel(m.atMs)}</p>}
                <div className={showName ? "msg first" : "msg"}>
                  {showName && (
                    <p className="msg-head">
                      <span className="msg-name">{m.fromMe ? "Sen" : friend.name}</span>
                      <span className="msg-time">{timeLabel(m.atMs)}</span>
                    </p>
                  )}
                  <p className="msg-text">{m.text}</p>
                </div>
              </div>
            );
          })
        )}
      </div>

      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <textarea
          className="input composer-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          rows={Math.min(5, Math.max(1, draft.split("\n").length))}
          maxLength={MAX_CHARS}
          placeholder={friend.online ? "Mesaj yaz" : `${friend.name} çevrimdışı, mesaj gitmez`}
          disabled={!friend.online}
          aria-label="Mesaj"
          autoFocus
        />
        <button
          className="send"
          disabled={!friend.online || !draft.trim() || sending}
          title="Gönder (Enter)"
          aria-label="Gönder"
        >
          <Icon name="send" size={16} />
        </button>
      </form>
    </main>
  );
}

function dayKey(ms: number): string {
  return new Date(ms).toDateString();
}

function dayLabel(ms: number): string {
  const day = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (day.toDateString() === today.toDateString()) return "Bugün";
  if (day.toDateString() === yesterday.toDateString()) return "Dün";
  return day.toLocaleDateString("tr-TR", { day: "numeric", month: "long", year: "numeric" });
}

function timeLabel(ms: number): string {
  return new Date(ms).toLocaleTimeString("tr-TR", { hour: "2-digit", minute: "2-digit" });
}
