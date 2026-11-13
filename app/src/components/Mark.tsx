import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";

/**
 * The window's title bar (Windows' own is turned off): the name, a screen's own controls,
 * then keep-on-top, minimize and close. Empty parts of the bar drag the window.
 */
export function TopBar({ children }: { children?: React.ReactNode }) {
  return (
    <div className="topbar" data-tauri-drag-region>
      <span className="wordmark" data-tauri-drag-region>
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
