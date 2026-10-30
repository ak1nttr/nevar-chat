import { useState } from "react";
import { api, AppState, CallPhase, Contact, describeError } from "../api";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";

interface Props {
  state: AppState;
  onSettings: () => void;
  toast: (text: string) => void;
}

export function Sidebar({ state, onSettings, toast }: Props) {
  const { me, contacts, call } = state;
  const byName = (a: Contact, b: Contact) => a.name.localeCompare(b.name, "tr");
  const friends = contacts.filter((c) => c.state === "friend");
  const online = friends.filter((c) => c.online).sort(byName);
  const offline = friends.filter((c) => !c.online).sort(byName);
  const incoming = contacts.filter((c) => c.state === "incoming");
  const outgoing = contacts.filter((c) => c.state === "outgoing");
  const run = (p: Promise<unknown>) => p.catch((e) => toast(describeError(e)));

  return (
    <aside className="sidebar">
      <header className="brand">
        <span className="wordmark">nevar</span>
        <span className={me.online ? "chip live" : "chip"}>
          <span className="dot" />
          {me.online ? "Bağlı" : "Bağlanıyor"}
        </span>
      </header>

      <div className="me">
        <Avatar id={me.id} name={me.name} size={36} />
        <div className="me-text">
          <strong>{me.name}</strong>
          <MyCode id={me.id} />
        </div>
        <button className="icon-button" onClick={onSettings} title="Ayarlar">
          <Icon name="settings" size={18} />
        </button>
      </div>

      <div className="lists">
        {incoming.length > 0 && (
          <Section label="İstekler" count={incoming.length} accent>
            {incoming.map((c) => (
              <div className="row" key={c.id}>
                <Avatar id={c.id} name={c.name} size={28} />
                <span className="row-name">{c.name}</span>
                <button className="square-button live" title="Kabul et" onClick={() => run(api.acceptRequest(c.id))}>
                  <Icon name="check" size={16} />
                </button>
                <button className="square-button" title="Reddet" onClick={() => run(api.declineRequest(c.id))}>
                  <Icon name="close" size={16} />
                </button>
              </div>
            ))}
          </Section>
        )}

        <Section label="Çevrimiçi" count={online.length}>
          {online.length === 0 && <p className="empty-line">Şu an kimse yok</p>}
          {online.map((c) => (
            <FriendRow
              key={c.id}
              contact={c}
              callPhase={call?.peer === c.id ? call.phase : undefined}
              busy={call != null}
              run={run}
            />
          ))}
        </Section>

        {offline.length > 0 && (
          <Section label="Çevrimdışı" count={offline.length}>
            {offline.map((c) => (
              <FriendRow key={c.id} contact={c} busy run={run} />
            ))}
          </Section>
        )}

        {outgoing.length > 0 && (
          <Section label="Gönderilen" count={outgoing.length}>
            {outgoing.map((c) => (
              <div className="row dim" key={c.id}>
                <Avatar id={c.id} name={c.name} size={28} />
                <span className="row-name">{c.name}</span>
                <span className="tag">Bekliyor</span>
                <button className="square-button reveal" title="İsteği geri çek" onClick={() => run(api.removeContact(c.id))}>
                  <Icon name="close" size={16} />
                </button>
              </div>
            ))}
          </Section>
        )}
      </div>

      <AddFriend toast={toast} />
    </aside>
  );
}

function Section(props: { label: string; count: number; accent?: boolean; children: React.ReactNode }) {
  return (
    <section className="section">
      <h2 className={props.accent ? "label accent" : "label"}>
        {props.label}
        <span>{props.count}</span>
      </h2>
      {props.children}
    </section>
  );
}

interface FriendRowProps {
  contact: Contact;
  /** Set when this friend is the one we're calling or talking to. */
  callPhase?: CallPhase;
  busy: boolean;
  run: (p: Promise<unknown>) => void;
}

function FriendRow({ contact: c, callPhase, busy, run }: FriendRowProps) {
  const [confirmRemove, setConfirmRemove] = useState(false);

  if (confirmRemove) {
    return (
      <div className="row confirm">
        <span className="row-name">{c.name} silinsin mi?</span>
        <button className="text-button danger" onClick={() => run(api.removeContact(c.id))}>
          Sil
        </button>
        <button className="text-button" onClick={() => setConfirmRemove(false)}>
          Vazgeç
        </button>
      </div>
    );
  }

  return (
    <div className={c.online ? "row" : "row dim"}>
      <Avatar id={c.id} name={c.name} size={28} online={c.online} />
      <span className="row-name">{c.name}</span>
      <button className="square-button reveal" title="Arkadaşlıktan çıkar" onClick={() => setConfirmRemove(true)}>
        <Icon name="close" size={14} />
      </button>
      {callPhase === "active" ? (
        <span className="tag live">Görüşmede</span>
      ) : callPhase ? (
        <span className="tag warn">{callPhase === "outgoing" ? "Aranıyor" : "Arıyor"}</span>
      ) : (
        c.online && (
          <button className="square-button call" title="Ara" disabled={busy} onClick={() => run(api.startCall(c.id))}>
            <Icon name="phone" size={16} />
          </button>
        )
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
    <button className="code-button" onClick={copy} title="Arkadaş kodunu kopyala">
      {copied ? (
        <span className="live-text">Kopyalandı</span>
      ) : (
        <>
          <code>{id.slice(0, 8)}</code>
          <Icon name="copy" size={13} />
        </>
      )}
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
      toast("İstek gönderildi. Kabul edince listende görünecek.");
    } catch (err) {
      toast(describeError(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="add-friend" onSubmit={submit}>
      <label className="label" htmlFor="friend-code">
        Arkadaş ekle
      </label>
      <div className="add-friend-row">
        <input
          id="friend-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="Kodu yapıştır"
          spellCheck={false}
        />
        <button className="square-button live solid" disabled={busy || !code.trim()} title="İstek gönder">
          <Icon name="plus" size={16} />
        </button>
      </div>
    </form>
  );
}
