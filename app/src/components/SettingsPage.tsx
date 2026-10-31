import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, AppState, Devices, MicTest as MicTestStats, Mode, Settings, describeError } from "../api";
import { vkFromKeyboard, vkFromMouse, vkLabel } from "../keys";
import { Icon } from "./Icon";
import { TopBar } from "./Mark";
import { displayLevel } from "./VoiceLane";

interface Props {
  state: AppState;
  onClose: () => void;
  toast: (text: string) => void;
}

/** Settings as a page in the same window; Esc or the "Arkadaşlar" link goes back. */
export function SettingsPage({ state, onClose, toast }: Props) {
  const [devices, setDevices] = useState<Devices | null>(null);
  const [name, setName] = useState(state.me.name);
  const [version, setVersion] = useState("");
  const settings = state.settings;

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  useEffect(() => {
    api.listDevices().then(setDevices).catch((e) => toast(describeError(e)));
  }, [toast]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const update = (patch: Partial<Settings>) =>
    api.setSettings({ ...settings, ...patch }).catch((e) => toast(describeError(e)));

  const saveName = () => {
    if (name.trim() && name.trim() !== state.me.name) {
      api.setName(name).catch((e) => toast(describeError(e)));
    }
  };

  return (
    <main className="screen settings">
      <TopBar />
      <div className="settings-body">
        <button className="crumb" onClick={onClose} title="Geri (Esc)">
          <Icon name="back" size={14} />
          Arkadaşlar
        </button>
        <h1 className="title">Ayarlar</h1>

        <Group label="Profil">
          <Field label="Görünen adın" hint="Arkadaşların seni bu adla görür.">
            <input
              className="input"
              value={name}
              maxLength={32}
              onChange={(e) => setName(e.target.value)}
              onBlur={saveName}
              onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            />
          </Field>
        </Group>

        <Group label="Ses cihazları">
          <Field label="Mikrofon">
            <DeviceSelect
              options={devices?.inputs ?? []}
              fallback={devices?.defaultInput}
              value={settings.mic}
              onChange={(mic) => update({ mic })}
            />
          </Field>
          <Field label="Hoparlör / kulaklık">
            <DeviceSelect
              options={devices?.outputs ?? []}
              fallback={devices?.defaultOutput}
              value={settings.speakers}
              onChange={(speakers) => update({ speakers })}
            />
          </Field>
          <p className="hint">
            Bluetooth kulaklığın mikrofonu, kulaklığı düşük kaliteli “telefon” moduna alır. Mümkünse ayrı bir mikrofon
            seç.
          </p>
          <Field label="Gelen ses" value={`%${Math.round(settings.output_volume * 100)}`}>
            <input
              className="slider"
              type="range"
              min={0}
              max={2}
              step={0.05}
              value={settings.output_volume}
              style={{ "--fill": `${(settings.output_volume / 2) * 100}%` } as React.CSSProperties}
              onChange={(e) => update({ output_volume: Number(e.target.value) })}
            />
          </Field>
          <MicTest inCall={state.call != null} toast={toast} />
        </Group>

        <Group label="Mikrofon ne zaman açık?">
          <div className="segmented">
            {(
              [
                ["voiceActivity", "Konuşunca"],
                ["pushToTalk", "Bas-konuş"],
                ["open", "Hep açık"],
              ] as [Mode, string][]
            ).map(([mode, label]) => (
              <button key={mode} className={settings.mode === mode ? "selected" : ""} onClick={() => update({ mode })}>
                {label}
              </button>
            ))}
          </div>
          {settings.mode === "voiceActivity" && (
            <p className="hint">Sesini algılayınca açılır, susunca kapanır. Klavye ve fan sesi geçmez.</p>
          )}
          {settings.mode === "pushToTalk" && (
            <KeyPicker value={settings.push_to_talk_key} onChange={(key) => update({ push_to_talk_key: key })} />
          )}
          {settings.mode === "open" && <p className="hint">Mikrofonun görüşme boyunca hep açık kalır.</p>}
        </Group>

        <Group label="Ses temizleme">
          <Switch
            label="Gürültü engelleme"
            hint="Klavye, fan ve oda sesini siler."
            checked={settings.noise_suppression}
            onChange={(noise_suppression) => update({ noise_suppression })}
          />
          <Switch
            label="Yankı engelleme"
            hint="Hoparlörden dinlerken karşı taraf kendi sesini duymaz."
            checked={settings.echo_cancellation}
            onChange={(echo_cancellation) => update({ echo_cancellation })}
          />
          <Switch
            label="Otomatik ses seviyesi"
            hint="Çok kısık ya da çok yüksek sesi dengeler."
            checked={settings.auto_gain}
            onChange={(auto_gain) => update({ auto_gain })}
          />
        </Group>

        <footer className="settings-foot">
          <span>nevar {version}</span>
          <span>
            Sorun olursa: <code>%APPDATA%\chat.nevar.app\nevar.log</code>
          </span>
        </footer>
      </div>
    </main>
  );
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <section className="group">
      <h2>{label}</h2>
      {children}
    </section>
  );
}

function Field(props: { label: string; hint?: string; value?: string; children: React.ReactNode }) {
  return (
    <label className="field">
      <span className="field-head">
        <span>{props.label}</span>
        {props.value && <span className="field-value">{props.value}</span>}
      </span>
      {props.children}
      {props.hint && <span className="hint">{props.hint}</span>}
    </label>
  );
}

function DeviceSelect(props: {
  options: string[];
  fallback: string | null | undefined;
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  const missing = props.value != null && props.options.length > 0 && !props.options.includes(props.value);
  return (
    <span className="select">
      <select value={props.value ?? ""} onChange={(e) => props.onChange(e.target.value || null)}>
        <option value="">Windows varsayılanı{props.fallback ? ` · ${props.fallback}` : ""}</option>
        {props.options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
        {missing && <option value={props.value!}>{props.value} (takılı değil)</option>}
      </select>
      <svg viewBox="0 0 12 12" aria-hidden="true">
        <path d="M3 4.5l3 3 3-3" />
      </svg>
    </span>
  );
}

/** Hear yourself the way friends will: through noise suppression, the codec and back. */
function MicTest({ inCall, toast }: { inCall: boolean; toast: (text: string) => void }) {
  const [running, setRunning] = useState(false);
  const [test, setTest] = useState<MicTestStats | null>(null);

  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => api.getMicTest().then(setTest).catch(() => {}), 100);
    return () => {
      clearInterval(timer);
      api.stopMicTest().catch(() => {});
    };
  }, [running]);

  // A call takes the mic over.
  useEffect(() => {
    if (inCall) setRunning(false);
  }, [inCall]);

  const toggle = async () => {
    if (running) {
      setRunning(false);
      setTest(null);
      return;
    }
    try {
      await api.startMicTest();
      setRunning(true);
    } catch (e) {
      toast(describeError(e));
    }
  };

  const level = displayLevel(test?.level ?? 0);
  return (
    <div className="mic-test">
      <div className="mic-test-row">
        <span className="field-head">Mikrofon testi</span>
        <button className="btn ghost" onClick={toggle} disabled={inCall && !running}>
          {running ? "Durdur" : "Kendini dinle"}
        </button>
      </div>
      {running && (
        <>
          <div className="mic-meter" aria-hidden="true">
            <span className={test?.transmitting ? "on" : ""} style={{ width: `${Math.round(level * 100)}%` }} />
          </div>
          <span className="hint">
            {test?.transmitting
              ? "Bu ses görüşmede karşıya gider."
              : "Şu an gönderilmez: sessiz, kapalı ya da bas-konuş tuşu basılı değil."}{" "}
            Sesini kulaklıkla dinle, hoparlörde yankı yapar.
          </span>
        </>
      )}
    </div>
  );
}

function Switch(props: { label: string; hint: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="switch-row">
      <span>
        <strong>{props.label}</strong>
        <small>{props.hint}</small>
      </span>
      <input
        type="checkbox"
        role="switch"
        className="switch"
        checked={props.checked}
        onChange={(e) => props.onChange(e.target.checked)}
      />
    </label>
  );
}

function KeyPicker({ value, onChange }: { value: number | null; onChange: (vk: number) => void }) {
  const [listening, setListening] = useState(false);

  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setListening(false);
      const vk = vkFromKeyboard(e);
      if (vk != null) {
        onChange(vk);
        setListening(false);
      }
    };
    const onMouse = (e: MouseEvent) => {
      const vk = vkFromMouse(e);
      if (vk != null) {
        e.preventDefault();
        onChange(vk);
        setListening(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onMouse, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onMouse, true);
    };
  }, [listening, onChange]);

  return (
    <div className="key-picker">
      <span>
        <strong>Bas-konuş tuşu</strong>
        <small>Oyundayken de çalışır, tuşu başka yerde kullanmana engel olmaz.</small>
      </span>
      <button className={listening ? "keycap listening" : "keycap"} onClick={() => setListening(true)}>
        {listening ? "Bir tuşa bas…" : vkLabel(value)}
      </button>
    </div>
  );
}
