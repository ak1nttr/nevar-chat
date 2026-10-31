// Windows notifications, for when nevar's window isn't the one being looked at.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

/** Whether nevar's window is open and focused right now. */
export async function windowFocused(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    return Boolean((await win.isVisible()) && (await win.isFocused()));
  } catch {
    return true;
  }
}

/** Shows a Windows notification. Asks for permission the first time; stays quiet if refused. */
export async function notify(title: string, body: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch {
    // Notifications are a nicety; a failure here must not break anything else.
  }
}
