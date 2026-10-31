import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";

// The app icon's speech bubble with its level bars, without the tile behind it.

export function Mark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="200 180 620 680" aria-hidden="true">
      <path
        d="M232 300c0-50 40-90 90-90h380c50 0 90 40 90 90v300c0 50-40 90-90 90H470l-150 120v-120h0c-49 0-88-40-88-90Z"
        fill="#f0a640"
      />
      <g stroke="#1c1406" strokeWidth="44" strokeLinecap="round">
        <path d="M372 410v80" />
        <path d="M452 350v200" />
        <path d="M532 390v120" />
        <path d="M612 330v240" />
        <path d="M692 420v60" />
      </g>
    </svg>
  );
}

/**
 * The window's title bar (Windows' own is turned off): mark and name, a screen's own controls,
 * then keep-on-top, minimize and close. Empty parts of the bar drag the window.
 */
export function TopBar({ children }: { children?: React.ReactNode }) {
  return (
    <div className="topbar" data-tauri-drag-region>
      <span className="wordmark" data-tauri-drag-region>
        <Mark />
        nevar
      </span>
      <span className="topbar-actions">
        {children}
        {children && <span className="topbar-sep" />}
        <WindowButtons />
      </span>
    </div>
  );
}

function WindowButtons() {
  const win = getCurrentWindow();
  // Read from the window each time, since every screen draws its own bar.
  const [pinned, setPinned] = useState(false);
  useEffect(() => {
    win.isAlwaysOnTop().then((on) => setPinned(!!on)).catch(() => {});
  }, [win]);

  const togglePin = () => {
    const next = !pinned;
    win.setAlwaysOnTop(next).then(() => setPinned(next)).catch(() => {});
  };
  const pinLabel = pinned ? "Üstte tutmayı bırak" : "Hep üstte tut";

  return (
    <>
      <button
        className={pinned ? "icon-btn on" : "icon-btn"}
        onClick={togglePin}
        title={pinLabel}
        aria-label={pinLabel}
        aria-pressed={pinned}
      >
        <Icon name="pin" size={15} />
      </button>
      <button className="icon-btn" onClick={() => win.minimize()} title="Küçült" aria-label="Küçült">
        <Icon name="minimize" size={15} />
      </button>
      {/* Closing hides to the tray (see lib.rs); quitting is in the tray menu. */}
      <button
        className="icon-btn close"
        onClick={() => win.close()}
        title="Kapat (nevar tepside açık kalır)"
        aria-label="Kapat"
      >
        <Icon name="close" size={15} />
      </button>
    </>
  );
}
