import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/inter-tight/300.css";
import "@fontsource/inter-tight/400.css";
import "@fontsource/inter-tight/500.css";
import "@fontsource/inter-tight/600.css";
import "@fontsource/inter-tight/700.css";
import "./styles.css";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { OverlayApp } from "./components/Overlay";

// Outside Tauri (plain browser during `pnpm dev`), fake the backend so the UI can be designed.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  await import("./dev/mock");
}

// The webview's own right-click menu (back, reload, inspect...) means nothing in an app.
// Keep it only where it does something: text fields and selected text, for copy and paste.
document.addEventListener("contextmenu", (e) => {
  const target = e.target as HTMLElement | null;
  const editable = target?.closest("input, textarea, [contenteditable='true']");
  const selected = (window.getSelection()?.toString() ?? "").length > 0;
  if (!editable && !selected) e.preventDefault();
});

// The same page runs in both windows: the app, and the overlay over games.
const overlay = getCurrentWebviewWindow().label === "overlay";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{overlay ? <OverlayApp /> : <App />}</React.StrictMode>,
);
