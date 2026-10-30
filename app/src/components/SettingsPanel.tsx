import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, AppState, Devices, Mode, Settings, describeError } from "../api";
import { vkFromKeyboard, vkFromMouse, vkLabel } from "../keys";
import { Icon } from "./Icon";

interface Props {
  state: AppState;
  onClose: () => void;
  toast: (text: string) => void;
}

export function SettingsPanel({ state, onClose, toast }: Props) {
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
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="panel" role="dialog" aria-label="Ayarlar">
        <header className="panel-head">
          <h2>Ayarlar</h2>
          <button className="icon-button" onClick={onClose} title="Kapat (Esc)">
            <Icon name="close" size={18} />
          </button>
        </header>

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
          <p className="note">
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
            <p className="note">Sesini algılayınca açılır, susunca kapanır. Klavye ve fan sesi geçmez.</p>
          )}
          {settings.mode === "pushToTalk" && (
            <KeyPicker value={settings.push_to_talk_key} onChange={(key) => update({ push_to_talk_key: key })} />
          )}
          {settings.mode === "open" && <p className="note">Mikrofonun görüşme boyunca hep açık kalır.</p>}
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

        <footer className="panel-foot">
          <span>nevar {version}</span>
          <span>Sorun olursa: %APPDATA%\chat.nevar.app\nevar.log</span>
        </footer>
      </div>
    </div>
  );
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <section className="group">
      <h3 className="label">{label}</h3>
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
      {props.hint && <span className="field-hint">{props.hint}</span>}
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
