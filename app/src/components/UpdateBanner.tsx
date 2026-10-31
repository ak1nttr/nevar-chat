import { useEffect, useState } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Check again every few hours, for people who leave nevar open for days. */
const CHECK_EVERY_MS = 4 * 60 * 60 * 1000;

/** Offers new versions from GitHub Releases. Never interrupts a call on its own. */
export function UpdateBanner({ inCall }: { inCall: boolean }) {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const look = () =>
      check()
        .then((u) => u && setUpdate(u))
        // Offline or GitHub unreachable: just try again later.
        .catch(() => {});
    look();
    const timer = setInterval(look, CHECK_EVERY_MS);
    return () => clearInterval(timer);
  }, []);

  if (!update) return null;

  const install = async () => {
    setFailed(false);
    setProgress(0);
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          setProgress(total ? done / total : 0);
        }
      });
      await relaunch();
    } catch {
      setProgress(null);
      setFailed(true);
    }
  };

  return (
    <div className="update-banner">
      {progress == null ? (
        <>
          <span>
            {failed ? "Güncelleme indirilemedi." : `Yeni sürüm hazır: ${update.version}`}
          </span>
          <button className="btn primary" onClick={install} disabled={inCall} title={inCall ? "Görüşme bitince güncelle" : undefined}>
            {failed ? "Tekrar dene" : "Güncelle"}
          </button>
        </>
      ) : (
        <span>Güncelleniyor… %{Math.round(progress * 100)}</span>
      )}
    </div>
  );
}
