import { useState } from "react";
import { api, AppState, Contact, describeError } from "../api";
import { Icon } from "./Icon";
import { TopBar } from "./Mark";

interface Props {
  state: AppState;
  onSettings: () => void;
  toast: (text: string) => void;
}

/** The page you see when not in a call: your friends, who can be called, and your code. */
export function Home({ state, onSettings, toast }: Props) {
  const { me, contacts } = state;
  const byName = (a: Contact, b: Contact) => a.name.localeCompare(b.name, "tr");
  const friends = contacts.filter((c) => c.state === "friend");
  const online = friends.filter((c) => c.online).sort(byName);
  const offline = friends.filter((c) => !c.online).sort(byName);
  const incoming = contacts.filter((c) => c.state === "incoming");
  const outgoing = contacts.filter((c) => c.state === "outgoing").sort(byName);
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  return (
    <main className="screen home">
      <TopBar>
        <button className="icon-btn" onClick={onSettings} title="Ayarlar" aria-label="Ayarlar">
          <Icon name="settings" size={16} />
        </button>
      </TopBar>

      <div className="home-body">
        <header className="page-head">
          <h1 className="title">Arkadaşlar</h1>
          <p className="status-line">
            <span className={me.online ? "dot online" : "dot"} />
            {me.online ? `Bağlı · ${online.length} kişi çevrimiçi` : "Bağlanıyor…"}
          </p>
        </header>

        {incoming.map((c) => (
          <div className="callout" key={c.id}>
            <p>
              <strong>{c.name}</strong> seni arkadaş olarak eklemek istiyor.
            </p>
            <div className="callout-actions">
              <button className="btn primary" onClick={() => run(api.acceptRequest(c.id))}>
                Kabul et
              </button>
              <button className="btn ghost" onClick={() => run(api.declineRequest(c.id))}>
                Reddet
              </button>
            </div>
          </div>
        ))}

        {friends.length + outgoing.length === 0 ? (
          <p className="empty">
            Henüz arkadaşın yok. Aşağıdaki kodunu bir arkadaşına gönder, onun kodunu da “Arkadaş ekle”ye yapıştır.
          </p>
        ) : (
          <ul className="friends">
            {[...online, ...offline].map((c) => (
              <FriendRow key={c.id} contact={c} run={run} />
            ))}
            {outgoing.map((c) => (
              <li className="row offline" key={c.id}>
                <span className="dot" />
                <span className="row-name">{c.name}</span>
                <button
                  className="icon-btn reveal"
                  title="İsteği geri çek"
                  aria-label={`${c.name} isteğini geri çek`}
                  onClick={() => run(api.removeContact(c.id))}
                >
                  <Icon name="close" size={14} />
                </button>
                <span className="row-note">yanıt bekleniyor</span>
              </li>
            ))}
          </ul>
        )}

        <AddFriend toast={toast} />
      </div>

      <MyCode id={me.id} />
    </main>
  );
}

function FriendRow({ contact: c, run }: { contact: Contact; run: (p: Promise<unknown>) => void }) {
  const [confirmRemove, setConfirmRemove] = useState(false);

  if (confirmRemove) {
    return (
      <li className="row confirm">
        <span className="row-name">{c.name} silinsin mi?</span>
        <button className="btn danger" onClick={() => run(api.removeContact(c.id))}>
          Sil
        </button>
        <button className="btn ghost" onClick={() => setConfirmRemove(false)}>
          Vazgeç
        </button>
      </li>
    );
  }

  return (
    <li className={c.online ? "row" : "row offline"}>
      <span className={c.online ? "dot online" : "dot"} />
      <span className="row-name">{c.name}</span>
      <button
        className="icon-btn reveal"
        title="Arkadaşlıktan çıkar"
        aria-label={`${c.name} arkadaşlıktan çıkar`}
        onClick={() => setConfirmRemove(true)}
      >
        <Icon name="close" size={14} />
      </button>
      {c.online ? (
        <button className="call-btn" onClick={() => run(api.startCall(c.id))} aria-label={`${c.name} kişisini ara`}>
          <Icon name="phone" size={14} />
          Ara
        </button>
      ) : (
        <span className="row-note">çevrimdışı</span>
      )}
    </li>
  );
}

function AddFriend({ toast }: { toast: (text: string) => void }) {
  const [open, setOpen] = useState(false);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!code.trim()) return;
    setBusy(true);
    try {
      await api.addFriend(code);
      setCode("");
      setOpen(false);
      toast("İstek gönderildi. Kabul edince listende görünecek.");
    } catch (err) {
      toast(describeError(err));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <div className="add">
        <button className="add-row" onClick={() => setOpen(true)}>
          <Icon name="plus" size={16} />
          Arkadaş ekle
        </button>
      </div>
    );
  }

  return (
    <form className="add add-form" onSubmit={submit}>
      <label htmlFor="friend-code">Arkadaşının kodunu yapıştır</label>
      <div className="add-form-row">
        <input
          id="friend-code"
          className="input"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
          placeholder="Kod"
          spellCheck={false}
          autoFocus
        />
        <button className="btn primary" disabled={busy || !code.trim()}>
          Ekle
        </button>
        <button type="button" className="btn ghost" onClick={() => setOpen(false)}>
          Vazgeç
        </button>
      </div>
    </form>
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
    <footer className="my-code">
      <span className="my-code-label">Senin kodun</span>
      <div className="my-code-row">
        <code title={id}>
          {id.slice(0, 8)}…{id.slice(-8)}
        </code>
        <button className="btn ghost" onClick={copy}>
          <Icon name={copied ? "check" : "copy"} size={14} />
          {copied ? "Kopyalandı" : "Kopyala"}
        </button>
      </div>
    </footer>
  );
}
