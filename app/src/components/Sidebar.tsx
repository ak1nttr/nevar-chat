import { useState } from "react";
import { api, AppState, Contact, describeError } from "../api";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";

interface Props {
  state: AppState;
  onSettings: () => void;
  toast: (text: string) => void;
}

export function Sidebar({ state, onSettings, toast }: Props) {
  const { me, contacts, call } = state;
  const friends = contacts
    .filter((c) => c.state === "friend")
    .sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name, "tr"));
  const incoming = contacts.filter((c) => c.state === "incoming");
  const outgoing = contacts.filter((c) => c.state === "outgoing");
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  return (
    <aside className="sidebar">
      <header className="me">
        <Avatar id={me.id} name={me.name} size={40} />
        <div className="me-text">
          <strong>{me.name}</strong>
          <span className={me.online ? "status on" : "status"}>
            {me.online ? "Çevrimiçi" : "Bağlanıyor…"}
          </span>
        </div>
        <button className="icon-button" onClick={onSettings} title="Ayarlar">
          <Icon name="settings" />
        </button>
      </header>

      <MyCode id={me.id} />

      <div className="lists">
        {incoming.length > 0 && (
          <section>
            <h2>İstekler</h2>
            {incoming.map((c) => (
              <div className="row" key={c.id}>
                <Avatar id={c.id} name={c.name} size={32} />
                <span className="row-name">{c.name}</span>
                <button className="icon-button accept" title="Kabul et" onClick={() => run(api.acceptRequest(c.id))}>
                  <Icon name="check" />
                </button>
                <button className="icon-button" title="Reddet" onClick={() => run(api.declineRequest(c.id))}>
                  <Icon name="close" />
                </button>
              </div>
            ))}
          </section>
        )}

        <section>
          <h2>
            Arkadaşlar <span className="count">{friends.filter((f) => f.online).length}/{friends.length}</span>
          </h2>
          {friends.length === 0 && <p className="hint">Henüz arkadaşın yok. Aşağıdan kodla ekleyebilirsin.</p>}
          {friends.map((c) => (
            <FriendRow key={c.id} contact={c} busy={call != null} run={run} />
          ))}
        </section>

        {outgoing.length > 0 && (
          <section>
            <h2>Bekleyen istekler</h2>
            {outgoing.map((c) => (
              <div className="row muted" key={c.id}>
                <Avatar id={c.id} name={c.name} size={32} />
                <span className="row-name">{c.name}</span>
                <button className="icon-button" title="İsteği geri çek" onClick={() => run(api.removeContact(c.id))}>
                  <Icon name="close" />
                </button>
              </div>
            ))}
          </section>
        )}
      </div>

      <AddFriend toast={toast} />
    </aside>
  );
}

function FriendRow({ contact: c, busy, run }: { contact: Contact; busy: boolean; run: (p: Promise<unknown>) => void }) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  return (
    <div className={c.online ? "row" : "row offline"}>
      <Avatar id={c.id} name={c.name} size={32} online={c.online} />
      <span className="row-name">{c.name}</span>
      {confirmRemove ? (
        <>
          <button className="text-button danger" onClick={() => run(api.removeContact(c.id))}>
            Kaldır
          </button>
          <button className="icon-button" title="Vazgeç" onClick={() => setConfirmRemove(false)}>
            <Icon name="back" />
          </button>
        </>
      ) : (
        <>
          <button
            className="icon-button call"
            title={c.online ? "Ara" : "Çevrimdışı"}
            disabled={!c.online || busy}
            onClick={() => run(api.startCall(c.id))}
          >
            <Icon name="phone" />
          </button>
          <button className="icon-button reveal" title="Arkadaşlıktan çıkar" onClick={() => setConfirmRemove(true)}>
            <Icon name="close" size={16} />
          </button>
        </>
      )}
    </div>
  );
}

function MyCode({ id }: { id: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(id);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <button className="my-code" onClick={copy} title="Arkadaşına gönder, seni eklesin">
      <span className="my-code-label">{copied ? "Kopyalandı" : "Arkadaş kodum"}</span>
      <code>{id.slice(0, 10)}…{id.slice(-6)}</code>
      <Icon name={copied ? "check" : "copy"} size={16} />
    </button>
  );
}

function AddFriend({ toast }: { toast: (text: string) => void }) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!code.trim()) return;
    setBusy(true);
    try {
      await api.addFriend(code);
      setCode("");
      toast("İstek gönderildi. Arkadaşın kabul edince listende görünecek.");
    } catch (err) {
      toast(describeError(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="add-friend" onSubmit={submit}>
      <input
        value={code}
        onChange={(e) => setCode(e.target.value)}
        placeholder="Arkadaşının kodunu yapıştır"
        spellCheck={false}
      />
      <button className="icon-button accent" disabled={busy || !code.trim()} title="İstek gönder">
        <Icon name="plus" />
      </button>
    </form>
  );
}
