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
        <header>
          <h2>Ayarlar</h2>
          <button className="icon-button" onClick={onClose} title="Kapat">
            <Icon name="close" />
          </button>
        </header>

        <section>
          <h3>Profil</h3>
          <label className="field">
            <span>Adın</span>
            <input
              value={name}
              maxLength={32}
              onChange={(e) => setName(e.target.value)}
              onBlur={saveName}
              onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            />
          </label>
        </section>

        <section>
          <h3>Ses cihazları</h3>
          <DeviceSelect
            label="Mikrofon"
            options={devices?.inputs ?? []}
            fallback={devices?.defaultInput}
            value={settings.mic}
            onChange={(mic) => update({ mic })}
          />
          <DeviceSelect
            label="Hoparlör / kulaklık"
            options={devices?.outputs ?? []}
            fallback={devices?.defaultOutput}
            value={settings.speakers}
            onChange={(speakers) => update({ speakers })}
          />
          <p className="hint">
            Bluetooth kulaklığın mikrofonunu kullanırsan Windows kulaklığı düşük kaliteli “telefon” moduna alır. Mümkünse
            ayrı bir mikrofon seç.
          </p>
          <label className="field">
            <span>Gelen ses seviyesi · %{Math.round(settings.output_volume * 100)}</span>
            <input
              type="range"
              min={0}
              max={2}
              step={0.05}
              value={settings.output_volume}
              onChange={(e) => update({ output_volume: Number(e.target.value) })}
            />
          </label>
        </section>

        <section>
          <h3>Ne zaman konuşuyorsun?</h3>
          <div className="segmented">
            {(
              [
                ["voiceActivity", "Sesimi algıla"],
                ["pushToTalk", "Bas-konuş"],
                ["open", "Hep açık"],
              ] as [Mode, string][]
            ).map(([mode, label]) => (
              <button key={mode} className={settings.mode === mode ? "selected" : ""} onClick={() => update({ mode })}>
                {label}
              </button>
            ))}
          </div>
          {settings.mode === "pushToTalk" && (
            <KeyPicker value={settings.push_to_talk_key} onChange={(key) => update({ push_to_talk_key: key })} />
          )}
          {settings.mode === "voiceActivity" && (
            <p className="hint">Konuştuğunu anlayınca mikrofonun açılır, susunca kapanır. Klavye ve fan sesi geçmez.</p>
          )}
        </section>

        <section>
          <h3>Ses temizleme</h3>
          <Toggle
            label="Gürültü engelleme"
            detail="Klavye, fan, oda sesini siler."
            checked={settings.noise_suppression}
            onChange={(noise_suppression) => update({ noise_suppression })}
          />
          <Toggle
            label="Yankı engelleme"
            detail="Hoparlörden dinlerken karşı tarafın kendi sesini duymasını önler."
            checked={settings.echo_cancellation}
            onChange={(echo_cancellation) => update({ echo_cancellation })}
          />
          <Toggle
            label="Otomatik ses seviyesi"
            detail="Sesin çok kısık ya da çok yüksekse dengeler."
            checked={settings.auto_gain}
            onChange={(auto_gain) => update({ auto_gain })}
          />
        </section>

        <footer className="panel-footer">
          nevar {version} · Sorun olursa %APPDATA%\chat.nevar.app\nevar.log dosyasını gönder.
        </footer>
      </div>
    </div>
  );
}

function DeviceSelect(props: {
  label: string;
  options: string[];
  fallback: string | null | undefined;
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  const missing = props.value != null && props.options.length > 0 && !props.options.includes(props.value);
  return (
    <label className="field">
      <span>{props.label}</span>
      <select value={props.value ?? ""} onChange={(e) => props.onChange(e.target.value || null)}>
        <option value="">Windows varsayılanı{props.fallback ? ` (${props.fallback})` : ""}</option>
        {props.options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
        {missing && <option value={props.value!}>{props.value} (takılı değil)</option>}
      </select>
    </label>
  );
}

function Toggle(props: { label: string; detail: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <span>
        <strong>{props.label}</strong>
        <small>{props.detail}</small>
      </span>
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
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
      <span>Bas-konuş tuşu</span>
      <button className={listening ? "key listening" : "key"} onClick={() => setListening(true)}>
        {listening ? "Bir tuşa ya da fare yan tuşuna bas…" : vkLabel(value)}
      </button>
    </div>
  );
}
